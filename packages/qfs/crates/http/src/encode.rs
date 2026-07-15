//! Response encoding (t32): content negotiation + the bounded result-size guard.
//!
//! The **JSON** response is the stable [result envelope](qfs_exec::RowSet) (blueprint §14,
//! ticket 20260703150300) — `{schema, rows, meta}` — the *same* serializer `qfs run --json` and
//! the MCP face use, so all three faces speak one shape. The **CSV** response (on
//! `Accept: text/csv` / `?format=csv`) is the flat rows via the codec registry (t15) — CSV has no
//! place to carry the envelope's schema/meta. No vendor type crosses this seam (blueprint §11).

use qfs_core::{CodecRegistry, RowBatch};
use qfs_exec::RowSet;

use crate::error::HttpError;
use crate::HttpRequest;

/// The default bounded in-memory result-row cap. A result exceeding this returns `413` rather
/// than buffering unboundedly (the ticket's bounded-buffer guard; pagination is a follow-up).
pub const DEFAULT_MAX_ROWS: usize = 10_000;

/// The negotiated content type: which codec format + its HTTP `Content-Type` header value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    /// `application/json` via the `json` codec (the default).
    Json,
    /// `text/csv` via the `csv` codec.
    Csv,
}

impl ContentType {
    /// The codec-registry format name (`"json"` / `"csv"`).
    #[must_use]
    pub fn codec_fmt(self) -> &'static str {
        match self {
            ContentType::Json => "json",
            ContentType::Csv => "csv",
        }
    }

    /// The HTTP `Content-Type` header value.
    #[must_use]
    pub fn header(self) -> &'static str {
        match self {
            ContentType::Json => "application/json",
            ContentType::Csv => "text/csv",
        }
    }
}

/// Negotiate the response codec from the request: `?format=csv` or `Accept: text/csv` selects
/// CSV; everything else defaults to JSON. The explicit `?format=` query param wins over the
/// `Accept` header (a deliberate caller override).
#[must_use]
pub fn negotiate(req: &HttpRequest) -> ContentType {
    if let Some(fmt) = req.query.get("format") {
        return match fmt.to_ascii_lowercase().as_str() {
            "csv" => ContentType::Csv,
            _ => ContentType::Json,
        };
    }
    match req.headers.get("accept") {
        Some(accept) if accept.to_ascii_lowercase().contains("text/csv") => ContentType::Csv,
        _ => ContentType::Json,
    }
}

/// Encode a [`RowSet`] to response bytes, enforcing the bounded result-size guard. **JSON** emits
/// the §14 result envelope (the shared serializer); **CSV** rebuilds a [`RowBatch`] and encodes it
/// through the registry codec (flat rows).
///
/// # Errors
///   * [`HttpError::Oversize`] (413) if the result exceeds `max_rows`.
///   * [`HttpError::Internal`] (500) if the codec is missing or encoding fails — sanitised,
///     never an upstream raw error.
pub fn encode_rows(
    rows: RowSet,
    content: ContentType,
    codecs: &CodecRegistry,
    max_rows: usize,
) -> Result<Vec<u8>, HttpError> {
    if rows.len() > max_rows {
        return Err(HttpError::Oversize { max: max_rows });
    }
    match content {
        // The stable result envelope — one serializer across `--json`, this endpoint, and MCP.
        ContentType::Json => serde_json::to_vec(&rows)
            .map_err(|_| HttpError::Internal("failed to encode response rows".to_string())),
        // CSV is the flat-rows projection via the codec (no envelope to carry schema/meta).
        ContentType::Csv => {
            let codec = codecs.resolve(content.codec_fmt()).map_err(|_| {
                HttpError::Internal(format!("no `{}` codec registered", content.codec_fmt()))
            })?;
            let batch = RowBatch::new(rows.schema, rows.rows);
            codec
                .encode(&batch)
                .map_err(|_| HttpError::Internal("failed to encode response rows".to_string()))
        }
    }
}
