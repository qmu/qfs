//! The Gmail **read composition** (t7): a `/mail/<label>` or `/mail/drafts` collection scan. Resolve
//! the path to a Gmail `q=` scope, list the matching message ids, and fetch each into the canonical
//! [`MailMessage`] rows. Pure-then-I/O over the mockable [`GmailClient`] — no vendor type crosses the
//! boundary, and the bearer never leaves the client. This is the read counterpart of the applier's
//! write leg; the binary's async `ReadDriver` adapter calls it (the same topology as the GitHub
//! driver's `read_rows`).

use qfs_types::{Predicate, Row, RowBatch, Value};

use crate::client::GmailClient;
use crate::error::GmailError;
use crate::path::MailPath;
use crate::query::build_query;
use crate::schema::{attachment_read_schema, label_listing_schema, MailMessage};

/// The fan-out ceiling for a collection scan when the engine still re-filters locally — over-fetch,
/// then the residual `WHERE`/`LIMIT` narrows. A pushed `LIMIT` tightens the fetch below this only
/// when the whole predicate pushed down (see [`read_rows`]).
const READ_CAP: u32 = 1_000;

/// Read a `/mail/<label>` or `/mail/drafts` collection into [`MailMessage`] rows, pushing the
/// `WHERE` into Gmail's `q=` search where it can (`from:`/`to:`/`subject:`/`after:`/`is:unread`/the
/// `label:` scope) and the `LIMIT` into the fetch cap where it is safe to.
///
/// The engine still re-applies the exact `WHERE`/`LIMIT` locally (over-fetch then filter, blueprint §7),
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
    // Single-node reads short-circuit the search/limit machinery: the mailbox ROOT lists labels
    // (gmail-ftp `ls /`), and a single MESSAGE node downloads that one message's row (gmail-ftp
    // `get`) — both advertised by `caps_for` (Root: Ls/Select; Message: Select) and now wired.
    let parsed = MailPath::parse_str(path)?;
    match &parsed {
        MailPath::Root => {
            let rows = client
                .list_labels()?
                .into_iter()
                .map(|name| Row::new(vec![Value::Text(name)]))
                .collect();
            return Ok(RowBatch::new(label_listing_schema(), rows));
        }
        MailPath::Message { id } => {
            let row = client.get_message(id)?.to_row();
            return Ok(RowBatch::new(MailMessage::schema(), vec![row]));
        }
        MailPath::Drafts => {
            // The drafts collection lists DRAFTS (`drafts.list`), not messages: each row's `id` is
            // the sendable **draft id**, so `/mail/drafts/<id> |> call mail.send` addresses a real
            // draft. The detail (from/subject/…) comes from the draft's message; the row's `id` is
            // overridden to the draft id (the message id is not sendable).
            let pushdown = build_query(None, predicate);
            let cap = match (limit, &pushdown.residual) {
                (Some(n), None) => u32::try_from(n).unwrap_or(READ_CAP).clamp(1, READ_CAP),
                _ => READ_CAP,
            };
            let refs = client.list_drafts(&pushdown.query, Some(cap))?;
            let mut rows = Vec::with_capacity(refs.len());
            for dr in &refs {
                let mut msg = client.get_message(&dr.message_id)?;
                msg.id = dr.id.clone();
                rows.push(msg.to_row());
            }
            return Ok(RowBatch::new(MailMessage::schema(), rows));
        }
        MailPath::Draft { id } => {
            // A single draft node (`/mail/drafts/<draft-id>`): resolve the draft id to its message,
            // then present the draft id as the row `id` (parity with the collection listing).
            let dr = client.get_draft(id)?;
            let mut msg = client.get_message(&dr.message_id)?;
            msg.id = dr.id.clone();
            return Ok(RowBatch::new(MailMessage::schema(), vec![msg.to_row()]));
        }
        MailPath::Attachment {
            message,
            attachment,
        } => {
            // gmail-ftp `get id:att:<msg>:<att>`: the bytes come from attachments.get, the
            // filename/mime/size from the owning message's part metadata (the API's attachment
            // endpoint returns only size + data). One combined row: filename, mime, size, content.
            //
            // The address `<att>` is a STABLE 0-based index `att<N>` (Gmail's `attachmentId` is
            // ephemeral — regenerated by every `messages.get` — so a literal id carried in from a
            // prior statement is already stale). We resolve the index against THIS `get_message`
            // and use the fresh id it carries, so the whole thing works in one plan. A non-`att<N>`
            // segment falls back to an explicit-id match (a within-session id still resolves).
            let msg = client.get_message(message)?;
            let meta = resolve_attachment(&msg.attachments, attachment).ok_or(
                GmailError::InvalidPath {
                    path: path.to_string(),
                    reason: "no attachment at that index (use att<N>, e.g. att0; see \
                             `… |> select attachments` for the order)",
                },
            )?;
            // The fresh id from this same `get_message` — never the (possibly stale) path segment.
            let bytes = client.get_attachment(message, &meta.attachment_id)?;
            let row = Row::new(vec![
                Value::Text(meta.filename.clone()),
                Value::Text(meta.mime.clone()),
                Value::Int(meta.size),
                Value::Bytes(bytes),
            ]);
            return Ok(RowBatch::new(attachment_read_schema(), vec![row]));
        }
        _ => {}
    }
    let pushdown = match parsed {
        MailPath::Label { name } => build_query(Some(&name), predicate),
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

/// Resolve an attachment path segment to its metadata. The canonical, stable form is `att<N>` — a
/// 0-based index into the message's attachments (in listing order), which survives the ephemeral
/// `attachmentId`. A segment that is not `att<N>` falls back to an explicit `attachment_id` match
/// (valid only within the same `get_message` session). Returns `None` for an out-of-range index or
/// an unknown id.
fn resolve_attachment<'a>(
    attachments: &'a [crate::schema::AttachmentMeta],
    segment: &str,
) -> Option<&'a crate::schema::AttachmentMeta> {
    if let Some(index) = segment
        .strip_prefix("att")
        .and_then(|n| n.parse::<usize>().ok())
    {
        return attachments.get(index);
    }
    attachments.iter().find(|a| a.attachment_id == segment)
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
            message_id: String::new(),
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
    fn reads_the_root_label_listing() {
        // gmail-ftp `ls /` parity: the mailbox root lists labels as `name` rows.
        let client = MockGmailClient::new().with_labels(vec![
            "INBOX".to_string(),
            "SENT".to_string(),
            "Work".to_string(),
        ]);
        let batch = read_rows(&client, "/mail", None, None).unwrap();
        assert_eq!(batch.schema.columns.len(), 1);
        assert_eq!(batch.schema.columns[0].name.as_str(), "name");
        let names: Vec<_> = batch
            .rows
            .iter()
            .map(|r| match &r.values[0] {
                Value::Text(s) => s.clone(),
                other => panic!("label name must be text, got {other:?}"),
            })
            .collect();
        assert_eq!(names, vec!["INBOX", "SENT", "Work"]);
        assert!(
            client
                .recorded()
                .iter()
                .any(|c| matches!(c, crate::client::RecordedCall::ListLabels)),
            "the root listing calls labels.list"
        );
    }

    #[test]
    fn reads_a_single_message_node_into_one_row() {
        // gmail-ftp `get` parity: a single message node downloads that message's row (headers +
        // snippet + attachments-as-nested-entries) instead of erroring `invalid_path`.
        let client = MockGmailClient::new().with_message(fixture_message("18f1a2b3", "Invoice 42"));
        let batch = read_rows(&client, "/mail/INBOX/18f1a2b3", None, None).unwrap();
        assert_eq!(batch.rows.len(), 1, "a message node is exactly one row");
        let idx = |name: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == name)
                .unwrap_or_else(|| panic!("column {name}"))
        };
        assert!(matches!(&batch.rows[0].values[idx("id")], Value::Text(s) if s == "18f1a2b3"));
        assert!(
            matches!(&batch.rows[0].values[idx("subject")], Value::Text(s) if s == "Invoice 42")
        );
        // The same path also resolves via the `id:` selector (label-independent addressing).
        let by_id = read_rows(&client, "id:18f1a2b3", None, None).unwrap();
        assert_eq!(by_id.rows.len(), 1);
    }

    #[test]
    fn reads_an_attachment_node_into_a_content_row() {
        // gmail-ftp `get id:att:<msg>:<att>` parity: the attachment node yields one row with the
        // filename/mime/size (from the message part) and the decoded `content` bytes. The node is
        // addressed by the STABLE index `att0` — the driver resolves the (ephemeral) id from this
        // read and fetches with it, so the user never handles the unstable id.
        let mut msg = fixture_message("18f1a2b3", "Invoice 42");
        msg.attachments = vec![crate::schema::AttachmentMeta {
            filename: "invoice.pdf".to_string(),
            mime: "application/pdf".to_string(),
            attachment_id: "ANGjdJ_ephemeral_id".to_string(),
            size: 5,
        }];
        // The mock keys the bytes by the id the read resolves from the message (never the path).
        let client = MockGmailClient::new().with_message(msg).with_attachment(
            "18f1a2b3",
            "ANGjdJ_ephemeral_id",
            b"hello".to_vec(),
        );
        let batch = read_rows(&client, "/mail/INBOX/18f1a2b3/att0", None, None).unwrap();
        assert_eq!(batch.rows.len(), 1);
        let idx = |name: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == name)
                .unwrap_or_else(|| panic!("column {name}"))
        };
        assert!(
            matches!(&batch.rows[0].values[idx("filename")], Value::Text(s) if s == "invoice.pdf")
        );
        assert!(
            matches!(&batch.rows[0].values[idx("mime")], Value::Text(s) if s == "application/pdf")
        );
        assert!(matches!(&batch.rows[0].values[idx("content")], Value::Bytes(b) if b == b"hello"));
        assert!(client
            .recorded()
            .iter()
            .any(|c| matches!(c, crate::client::RecordedCall::GetAttachment { .. })));
    }

    #[test]
    fn att_index_resolves_a_fresh_id_and_out_of_range_fails_closed() {
        // The live L67 root cause: Gmail's `attachmentId` is ephemeral (a new one per
        // `messages.get`), so it cannot be carried across statements. The stable address is the
        // 0-based index `att<N>`; the driver resolves the id from THIS read and fetches with it, so
        // a one-statement `… FROM /mail/<label>/<msg>/att0 |> insert into /drive/…` works.
        let mut msg = fixture_message("m9", "Two files");
        msg.attachments = vec![
            crate::schema::AttachmentMeta {
                filename: "a.pdf".to_string(),
                mime: "application/pdf".to_string(),
                attachment_id: "id-fresh-A".to_string(),
                size: 1,
            },
            crate::schema::AttachmentMeta {
                filename: "b.csv".to_string(),
                mime: "text/csv".to_string(),
                attachment_id: "id-fresh-B".to_string(),
                size: 1,
            },
        ];
        let client = MockGmailClient::new().with_message(msg).with_attachment(
            "m9",
            "id-fresh-B",
            b"beta".to_vec(),
        );

        // att1 addresses the SECOND attachment; the driver fetches with the id it resolved from the
        // message (`id-fresh-B`), never the path segment `att1`.
        let batch = read_rows(&client, "/mail/INBOX/m9/att1", None, None).unwrap();
        let idx = |n: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == n)
                .unwrap()
        };
        assert!(matches!(&batch.rows[0].values[idx("filename")], Value::Text(s) if s == "b.csv"));
        assert!(matches!(&batch.rows[0].values[idx("content")], Value::Bytes(b) if b == b"beta"));
        assert!(client.recorded().iter().any(|c| matches!(
            c,
            crate::client::RecordedCall::GetAttachment { attachment_id, .. } if attachment_id == "id-fresh-B"
        )));

        // An out-of-range index fails closed (no such attachment), not a silent empty read.
        let err = read_rows(&client, "/mail/INBOX/m9/att5", None, None).unwrap_err();
        assert_eq!(err.code(), "invalid_path");
    }

    #[test]
    fn drafts_collection_exposes_the_sendable_draft_id_not_the_message_id() {
        // `/mail/drafts` lists via `drafts.list`: each row's `id` is the DRAFT id (the sendable
        // identity `/mail/drafts/<id> |> call mail.send` addresses), NOT the message id. The detail
        // (subject/from/…) still comes from the draft's message.
        let client = MockGmailClient::new()
            .with_draft("d1", "m1")
            .with_message(fixture_message("m1", "Q3 draft"));
        let batch = read_rows(&client, "/mail/drafts", None, None).unwrap();
        assert_eq!(batch.rows.len(), 1);
        let idx = |name: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == name)
                .unwrap_or_else(|| panic!("column {name}"))
        };
        assert!(
            matches!(&batch.rows[0].values[idx("id")], Value::Text(s) if s == "d1"),
            "the drafts listing exposes the draft id, not the message id"
        );
        assert!(matches!(&batch.rows[0].values[idx("subject")], Value::Text(s) if s == "Q3 draft"));
        assert!(
            client
                .recorded()
                .iter()
                .any(|c| matches!(c, crate::client::RecordedCall::ListDrafts { .. })),
            "the drafts scan lists drafts (not messages)"
        );
    }

    #[test]
    fn a_single_draft_node_reads_by_its_draft_id() {
        // `/mail/drafts/<draft-id>` resolves the draft id to its message and presents the draft id
        // as the row `id` (parity with the collection listing).
        let client = MockGmailClient::new()
            .with_draft("d1", "m1")
            .with_message(fixture_message("m1", "Q3 draft"));
        let batch = read_rows(&client, "/mail/drafts/d1", None, None).unwrap();
        assert_eq!(batch.rows.len(), 1);
        let id_idx = batch
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "id")
            .expect("id column");
        assert!(matches!(&batch.rows[0].values[id_idx], Value::Text(s) if s == "d1"));
        assert!(client
            .recorded()
            .iter()
            .any(|c| matches!(c, crate::client::RecordedCall::GetDraft { .. })));
    }

    #[test]
    fn an_attachment_node_with_no_such_attachment_fails_closed() {
        // A message with no matching attachment id is a structured invalid_path, not a panic.
        let client = MockGmailClient::new().with_message(fixture_message("18f1a2b3", "Invoice 42"));
        let err = read_rows(&client, "/mail/INBOX/18f1a2b3/att1", None, None).unwrap_err();
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
