//! Bounded endpoint result paging (blueprint §14 contract 3, ticket 20260704152639).
//!
//! An endpoint result is requestable in **pages** via the `limit`/`offset` query knobs — the SAME
//! vocabulary the result envelope's `meta.{limit,offset,truncated}` carries (ticket 20260703150300),
//! so a client reads one paging shape everywhere and there is no second dialect. Cursor paging was
//! rejected: qfs sources cannot generally guarantee the stable sort key an honest cursor needs.
//!
//! Paging is a **post-slice** over the already-evaluated result, so it composes with a pushed-down
//! `LIMIT` in the endpoint's own query without double-truncation surprises: the query's `LIMIT`
//! bounds what the source returns; `offset`/`limit` then page over that bounded set, and
//! `meta.truncated` reports honestly whether the page cut anything.

use qfs_exec::{ResultMeta, RowSet};

use crate::error::HttpError;
use crate::params::BindError;
use crate::HttpRequest;

/// The reserved paging query keys — negotiation knobs, never endpoint params.
pub const LIMIT_KEY: &str = "limit";
/// The reserved paging offset key.
pub const OFFSET_KEY: &str = "offset";

/// Parse a non-negative integer paging param from the query string. A present-but-malformed value
/// is a 400 (`bind`) naming the param; an absent value is `Ok(None)`.
fn parse_param(req: &HttpRequest, key: &str) -> Result<Option<i64>, HttpError> {
    match req.query.get(key) {
        None => Ok(None),
        Some(raw) => match raw.parse::<i64>() {
            Ok(n) if n >= 0 => Ok(Some(n)),
            _ => Err(HttpError::Bind(BindError::type_mismatch(
                key,
                format!("`{key}` must be a non-negative integer (got `{raw}`)"),
            ))),
        },
    }
}

/// Apply `?limit`/`?offset` paging to an evaluated [`RowSet`], recording the honest bound in the
/// envelope's `meta`. Returns the RowSet unchanged (no `meta` paging fields) when neither knob is
/// present, so a non-paged request is byte-identical to before.
///
/// Semantics: `offset` (default 0) drops the leading rows; `limit` caps the remaining rows.
/// `truncated` is set iff rows remained beyond the returned page.
///
/// # Errors
/// [`HttpError::Bind`] (400) if `limit`/`offset` is present but not a non-negative integer.
pub fn apply(rows: RowSet, req: &HttpRequest) -> Result<RowSet, HttpError> {
    let limit = parse_param(req, LIMIT_KEY)?;
    let offset = parse_param(req, OFFSET_KEY)?;
    if limit.is_none() && offset.is_none() {
        return Ok(rows);
    }

    let total = rows.rows.len();
    // `offset` past the end yields an empty page (not an error) — a client walking pages stops here.
    let start = offset.map_or(0, |o| (o as usize).min(total));
    let available = total - start;
    let take = limit.map_or(available, |l| (l as usize).min(available));
    let truncated = start + take < total;

    let RowSet { schema, rows, .. } = rows;
    let paged: Vec<_> = rows.into_iter().skip(start).take(take).collect();

    Ok(RowSet {
        schema,
        rows: paged,
        meta: ResultMeta {
            truncated,
            limit,
            offset,
            affected: None,
        },
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_core::{Column, ColumnType, Row, RowBatch, Schema, Value};

    fn rowset(n: usize) -> RowSet {
        let schema = Schema::new(vec![Column::new("id", ColumnType::Int, false)]);
        let rows = (0..n)
            .map(|i| Row::new(vec![Value::Int(i as i64)]))
            .collect();
        RowSet::from_batch(RowBatch::new(schema, rows))
    }

    fn req(query: &[(&str, &str)]) -> HttpRequest {
        let mut r = HttpRequest::new(crate::Method::Get, "/x");
        for (k, v) in query {
            r = r.with_query(*k, *v);
        }
        r
    }

    #[test]
    fn no_paging_params_is_unchanged() {
        let paged = apply(rowset(5), &req(&[])).unwrap();
        assert_eq!(paged.rows.len(), 5);
        assert!(!paged.meta.truncated);
        assert!(paged.meta.limit.is_none() && paged.meta.offset.is_none());
    }

    #[test]
    fn limit_bounds_and_flags_truncation() {
        let paged = apply(rowset(10), &req(&[("limit", "3")])).unwrap();
        assert_eq!(paged.rows.len(), 3);
        assert!(paged.meta.truncated, "7 rows remained beyond the page");
        assert_eq!(paged.meta.limit, Some(3));
        // The first page is ids 0,1,2.
        assert_eq!(paged.rows[0].values[0], Value::Int(0));
    }

    #[test]
    fn offset_and_limit_page_the_middle() {
        let paged = apply(rowset(10), &req(&[("limit", "3"), ("offset", "6")])).unwrap();
        assert_eq!(paged.rows.len(), 3); // ids 6,7,8
        assert_eq!(paged.rows[0].values[0], Value::Int(6));
        assert!(paged.meta.truncated, "id 9 remains beyond the page");
        assert_eq!(paged.meta.offset, Some(6));
    }

    #[test]
    fn last_page_is_not_truncated() {
        let paged = apply(rowset(10), &req(&[("limit", "5"), ("offset", "5")])).unwrap();
        assert_eq!(paged.rows.len(), 5); // ids 5..=9
        assert!(
            !paged.meta.truncated,
            "nothing remains beyond the last page"
        );
    }

    #[test]
    fn offset_past_end_is_empty_not_error() {
        let paged = apply(rowset(3), &req(&[("offset", "100")])).unwrap();
        assert_eq!(paged.rows.len(), 0);
        assert!(!paged.meta.truncated);
    }

    #[test]
    fn malformed_limit_is_a_bind_error() {
        let err = apply(rowset(3), &req(&[("limit", "-2")])).unwrap_err();
        assert_eq!(err.status(), 400);
        let err = apply(rowset(3), &req(&[("offset", "abc")])).unwrap_err();
        assert_eq!(err.status(), 400);
    }
}
