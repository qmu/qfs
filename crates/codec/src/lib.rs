//! `cfs-codec` — the codec contract (RFD-0001 §4).
//!
//! Codecs bridge blob ↔ relational: `DECODE fmt` / `ENCODE fmt` for `json, yaml,
//! toml, csv, markdown+frontmatter`. They are **pure `bytes ↔ rows`** and work on
//! *any* blob source (FS, S3, git, Drive, REST response) — which is why this is a
//! separate trait and registry from [`cfs_driver::Driver`], composing over blob
//! sources independent of driver identity (boundary B-codec).
//!
//! ## Purity invariant (fidelity guard G3, boundary B4)
//! [`Codec::decode`] / [`Codec::encode`] take `&self` and owned byte/row data and
//! return owned data or a [`CfsError`]. No `&mut self`, no future, no I/O. The
//! in-crate test [`tests::dummy_codec_is_pure`] proves a no-I/O codec instantiates.
//!
//! ## wasm-friendliness (boundary guard B7)
//! No threads, no `std::fs`, no sockets — codecs are pure transforms.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

// Re-export the shared error so codec consumers see it without naming cfs-driver.
pub use cfs_driver::CfsError;

/// A single decoded value (RFD-0001 §4 data model). E0 ships a minimal owned set;
/// nested `struct`/`array` column types and typed columns land in E3.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// SQL-style absence of a value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A 64-bit signed integer.
    Int(i64),
    /// A 64-bit float.
    Float(f64),
    /// An owned UTF-8 string.
    Text(String),
}

/// A single row: an ordered list of column values. Owned data only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// The column values, in column order.
    pub values: Vec<Value>,
}

/// A batch of rows with their column names — the relational side of a codec
/// (RFD-0001 §4). Owned data only; this is the DTO that crosses the codec boundary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowBatch {
    /// Column names, in column order.
    pub columns: Vec<String>,
    /// The rows.
    pub rows: Vec<Row>,
}

/// The pure `bytes ↔ rows` codec trait (RFD-0001 §4).
pub trait Codec: Send + Sync {
    /// The format identifier, e.g. `"json"`, `"yaml"`, `"md+frontmatter"`.
    fn fmt(&self) -> &str;

    /// Decode bytes into a [`RowBatch`]. Pure: no I/O, no side effects.
    ///
    /// # Errors
    /// Returns [`CfsError`] if the bytes are not valid for this format.
    fn decode(&self, bytes: &[u8]) -> Result<RowBatch, CfsError>;

    /// Encode a [`RowBatch`] into bytes. Pure: no I/O, no side effects.
    ///
    /// # Errors
    /// Returns [`CfsError`] if the batch cannot be encoded in this format.
    fn encode(&self, rows: &RowBatch) -> Result<Vec<u8>, CfsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-I/O dummy codec: a trivial line-per-row text format, purely in memory.
    struct DummyCodec;

    impl Codec for DummyCodec {
        fn fmt(&self) -> &str {
            "dummy"
        }

        fn decode(&self, bytes: &[u8]) -> Result<RowBatch, CfsError> {
            // Pure in-memory transform; no filesystem/network/clock access.
            let text = String::from_utf8_lossy(bytes);
            let rows = text
                .lines()
                .map(|line| Row {
                    values: vec![Value::Text(line.to_string())],
                })
                .collect();
            Ok(RowBatch {
                columns: vec!["line".to_string()],
                rows,
            })
        }

        fn encode(&self, batch: &RowBatch) -> Result<Vec<u8>, CfsError> {
            let mut out = String::new();
            for row in &batch.rows {
                if let Some(Value::Text(t)) = row.values.first() {
                    out.push_str(t);
                    out.push('\n');
                }
            }
            Ok(out.into_bytes())
        }
    }

    /// G3 — the codec purity proof. A no-I/O codec instantiates and round-trips.
    #[test]
    fn dummy_codec_is_pure() {
        let c = DummyCodec;
        assert_eq!(c.fmt(), "dummy");
        let decoded = c.decode(b"a\nb").unwrap();
        assert_eq!(decoded.rows.len(), 2);
        let encoded = c.encode(&decoded).unwrap();
        assert_eq!(encoded, b"a\nb\n");
    }

    /// The codec is object-safe (`dyn Codec`) — required for `CodecRegistry`
    /// storing `Arc<dyn Codec>` (G2).
    #[test]
    fn codec_is_object_safe() {
        let c: std::sync::Arc<dyn Codec> = std::sync::Arc::new(DummyCodec);
        assert_eq!(c.fmt(), "dummy");
    }
}
