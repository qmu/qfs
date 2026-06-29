//! The Gmail **read composition** (t7): a `/mail/<label>` or `/mail/drafts` collection scan. Resolve
//! the path to a Gmail `q=` scope, list the matching message ids, and fetch each into the canonical
//! [`MailMessage`] rows. Pure-then-I/O over the mockable [`GmailClient`] — no vendor type crosses the
//! boundary, and the bearer never leaves the client. This is the read counterpart of the applier's
//! write leg; the binary's async `ReadDriver` adapter calls it (the same topology as the GitHub
//! driver's `read_rows`).

use qfs_types::{Predicate, RowBatch};

use crate::client::GmailClient;
use crate::error::GmailError;
use crate::path::MailPath;
use crate::query::build_query;
use crate::schema::MailMessage;

/// The fan-out ceiling for a collection scan when the engine still re-filters locally — over-fetch,
/// then the residual `WHERE`/`LIMIT` narrows. A pushed `LIMIT` tightens the fetch below this only
/// when the whole predicate pushed down (see [`read_rows`]).
const READ_CAP: u32 = 1_000;

/// Read a `/mail/<label>` or `/mail/drafts` collection into [`MailMessage`] rows, pushing the
/// `WHERE` into Gmail's `q=` search where it can (`from:`/`to:`/`subject:`/`after:`/`is:unread`/the
/// `label:` scope) and the `LIMIT` into the fetch cap where it is safe to.
///
/// The engine still re-applies the exact `WHERE`/`LIMIT` locally (over-fetch then filter, RFD §6),
/// so Gmail's looser field operators never return wrong rows; the `q=` is a backend pre-filter that
/// narrows the fetch. The pushed `LIMIT` is applied to the fetch cap **only** when nothing is left
/// as a local residual — with a residual, a tight cap would under-fetch (drop rows that survive the
/// local filter), so the fetch stays at [`READ_CAP`] and the engine applies the `LIMIT`.
///
/// # Errors
/// [`GmailError`] when the path is not a readable collection, or on an auth / transport / decode
/// failure from the client (secret-free, carrying the stable `code`).
pub fn read_rows(
    client: &dyn GmailClient,
    path: &str,
    predicate: Option<&Predicate>,
    limit: Option<u64>,
) -> Result<RowBatch, GmailError> {
    let pushdown = match MailPath::parse_str(path)? {
        MailPath::Label { name } => build_query(Some(&name), predicate),
        MailPath::Drafts => {
            // The drafts scope is `in:draft`, not a `label:` term — build the predicate query, then
            // prepend the scope.
            let mut pd = build_query(None, predicate);
            pd.query = if pd.query.is_empty() {
                "in:draft".to_string()
            } else {
                format!("in:draft {}", pd.query)
            };
            pd
        }
        _ => {
            return Err(GmailError::InvalidPath {
                path: path.to_string(),
                reason: "read a /mail/<label> or /mail/drafts collection",
            })
        }
    };
    // Push the planner LIMIT into the fetch cap only when the whole predicate pushed down (no
    // residual); otherwise over-fetch up to READ_CAP and let the engine apply the LIMIT after its
    // local re-filter. Either way READ_CAP is the hard ceiling.
    let cap = match (limit, &pushdown.residual) {
        (Some(n), None) => u32::try_from(n).unwrap_or(READ_CAP).clamp(1, READ_CAP),
        _ => READ_CAP,
    };
    let page = client.search_message_ids(&pushdown.query, Some(cap))?;
    let mut rows = Vec::with_capacity(page.ids.len());
    for id in &page.ids {
        rows.push(client.get_message(id)?.to_row());
    }
    Ok(RowBatch::new(MailMessage::schema(), rows))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::client::{MessageIdPage, MockGmailClient};
    use qfs_types::Value;

    fn fixture_message(id: &str, subject: &str) -> MailMessage {
        MailMessage {
            id: id.to_string(),
            thread_id: "t1".to_string(),
            label_ids: vec!["INBOX".to_string()],
            date: 1_700_000_000,
            from: "alice@example.com".to_string(),
            subject: subject.to_string(),
            snippet: "preview".to_string(),
            attachments: Vec::new(),
        }
    }

    #[test]
    fn reads_a_label_collection_into_message_rows() {
        let client = MockGmailClient::new()
            .with_search_page(MessageIdPage {
                ids: vec!["m1".to_string()],
                next_page_token: None,
            })
            .with_message(fixture_message("m1", "Invoice 42"));
        let batch = read_rows(&client, "/mail/INBOX", None, None).unwrap();
        assert_eq!(batch.rows.len(), 1);
        let subj = batch
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "subject")
            .expect("subject column");
        assert!(matches!(&batch.rows[0].values[subj], Value::Text(s) if s == "Invoice 42"));
    }

    #[test]
    fn a_message_node_is_not_a_collection_read() {
        let client = MockGmailClient::new();
        let err = read_rows(&client, "/mail/INBOX/18f1a2b3", None, None).unwrap_err();
        assert_eq!(err.code(), "invalid_path");
    }

    /// Extract the single recorded Gmail search call's `(query, max_results)`.
    fn recorded_search(client: &MockGmailClient) -> (String, Option<u32>) {
        client
            .recorded()
            .into_iter()
            .find_map(|c| match c {
                crate::client::RecordedCall::Search { query, max_results } => {
                    Some((query, max_results))
                }
                _ => None,
            })
            .expect("a search call was recorded")
    }

    #[test]
    fn pushes_the_where_into_the_gmail_search_query_and_keeps_a_lossy_residual_uncapped() {
        use qfs_types::{CmpOp, ColRef, Literal, Predicate};
        let client = MockGmailClient::new().with_search_page(MessageIdPage {
            ids: Vec::new(),
            next_page_token: None,
        });
        // `subject = 'Invoice'` is a LOSSY Gmail `subject:` pre-filter → kept as residual.
        let pred = Predicate::Cmp(
            ColRef::col("subject"),
            CmpOp::Eq,
            Literal::Text("Invoice".to_string()),
        );
        read_rows(&client, "/mail/INBOX", Some(&pred), Some(5)).unwrap();
        let (query, max_results) = recorded_search(&client);
        assert!(query.contains("label:INBOX"), "query: {query}");
        assert!(query.contains("subject:Invoice"), "query: {query}");
        // A residual remains, so the LIMIT is NOT pushed to the fetch — the engine applies it.
        assert_eq!(max_results, Some(READ_CAP));
    }

    #[test]
    fn pushes_the_limit_into_the_fetch_when_no_residual_remains() {
        let client = MockGmailClient::new().with_search_page(MessageIdPage {
            ids: Vec::new(),
            next_page_token: None,
        });
        // A bare label scan with no `WHERE` leaves no residual, so the LIMIT is safe to push.
        read_rows(&client, "/mail/INBOX", None, Some(5)).unwrap();
        let (query, max_results) = recorded_search(&client);
        assert!(query.contains("label:INBOX"), "query: {query}");
        assert_eq!(max_results, Some(5));
    }
}
