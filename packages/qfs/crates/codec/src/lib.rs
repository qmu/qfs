//! `qfs-codec` — the codec contract (blueprint §4).
//!
//! Codecs bridge blob ↔ relational: `DECODE fmt` / `ENCODE fmt` for `json, yaml,
//! toml, csv, markdown+frontmatter`. They are **pure `bytes ↔ rows`** and work on
//! *any* blob source (FS, S3, git, Drive, REST response) — which is why this is a
//! separate trait and registry from [`qfs_driver::Driver`], composing over blob
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

mod codecs;
mod convert;
mod nested;

// Re-export the shared error so codec consumers see it without naming qfs-driver.
pub use qfs_driver::CfsError;

// Re-export the canonical row model from qfs-types (t05). E0 shipped placeholder
// `Value`/`Row`/`RowBatch` here; the canonical typed model (scalars, struct/array,
// json, explicit nulls, schema descriptor) now lives in the leaf `qfs-types` crate,
// and codecs target it so the `bytes <-> rows` boundary speaks the one row model.
pub use qfs_types::{Row, RowBatch, Schema, Value};

// The six builtin codecs (t15) and the canonical `builtin_codecs()` set that
// `CodecRegistry::with_builtins()` loads. Each codec maps `bytes <-> RowBatch` purely.
pub use codecs::{
    builtin_codecs, CsvCodec, JsonCodec, JsonlCodec, MarkdownFrontmatterCodec, TomlCodec, YamlCodec,
};

// The markdown tree interpretation (blueprint §13b): the `documents`/`links` named relations of
// the `md` codec, rehomed from `qfs-driver-markdown` into the codec layer. Public so the codec
// application can reach the relations and the row-equivalence test can pin them to the driver.
pub use codecs::{
    decode_markdown_relation, documents_schema as markdown_documents_schema,
    links_schema as markdown_links_schema, parse_document as parse_markdown_document,
    relation_schema as markdown_relation_schema, MarkdownRelation, ParsedDocument, ParsedLink,
};

// The runtime nested-data operators (blueprint §4): value-level `EXPAND` and `a.b.c` path
// access over the struct/array model. The type-level twins live in `qfs_types::Schema`.
pub use nested::{access, access_row, expand};

// The qfs-`Value` → JSON node converter (the clean, untagged JSON the wire speaks — NOT the
// serde-derived tagged form). Used by the §13 declared-map write path to encode an evaluated
// `Value::Struct` wire body into the JSON bytes the applier POSTs.
pub use convert::value_to_json;

/// The pure `bytes ↔ rows` codec trait (blueprint §4).
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

    /// The codec's **named relations** (blueprint §13b): a codec may interpret one blob as more
    /// than one relation of the same format — `decode md.documents`, `decode md.links` — each
    /// reached by a relation-qualified format token. The default is a single unnamed relation
    /// (`&[]`): a `decode <fmt>` with no relation qualifier yields [`Codec::decode`], and a
    /// relation qualifier over such a codec is a usage error. A multi-relation codec (the `md`
    /// codec) overrides this to declare its relations, primary first.
    fn relations(&self) -> &'static [&'static str] {
        &[]
    }

    /// Decode a blob into the rows of one **named relation** (blueprint §13b), given the source's
    /// root-relative `source_path` provenance (some relations — a link graph's `target_doc` —
    /// normalize against it). `relation = None` selects the codec's primary/unnamed relation
    /// ([`Codec::decode`]); `Some(name)` selects a declared relation. The default handles the
    /// single-relation case; a multi-relation codec overrides it.
    ///
    /// # Errors
    /// [`CfsError`] if the bytes are invalid, or if a relation is named on a single-relation
    /// codec / is not one this codec declares.
    fn decode_relation(
        &self,
        relation: Option<&str>,
        bytes: &[u8],
        source_path: Option<&str>,
    ) -> Result<RowBatch, CfsError> {
        let _ = source_path;
        match relation {
            None => self.decode(bytes),
            Some(name) => Err(CfsError::Decode {
                fmt: "codec",
                detail: format!(
                    "the `{}` codec has no relation `{name}` (it yields a single relation)",
                    self.fmt()
                ),
            }),
        }
    }
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
            let schema = Schema::new(vec![qfs_types::Column::new(
                "line",
                qfs_types::ColumnType::Text,
                false,
            )]);
            Ok(RowBatch { schema, rows })
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
