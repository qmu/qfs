//! Owned result DTOs the executor returns and the CLI renders: [`RowSet`] (read results) and
//! [`PlanPreview`] (effect dry-run). No vendor types cross this seam (blueprint §11) — these are the
//! only shapes the renderer (`output.rs`) ever sees, alongside the owned
//! [`ExecError`](crate::ExecError).
//!
//! ## The result envelope (blueprint §14, ticket 20260703150300)
//! A [`RowSet`] serializes to the **stable, schema-carrying result envelope** — one shape for all
//! three faces (`qfs run --json`, the HTTP endpoint, the MCP result payload):
//! ```json
//! {
//!   "schema": [ {"name":"date","type":"timestamp"}, {"name":"content","type":"bytes"} ],
//!   "rows":   [ {"date":1751600000000, "content":"aGVsbG8="} ],
//!   "meta":   {"row_count":1, "truncated":false, "limit":null, "offset":null, "affected":null}
//! }
//! ```
//! - `rows` stays an array of **objects** keyed by column name (an agent reads `row.subject`
//!   directly) — unchanged from the t29 contract, so existing `json["rows"]` consumers keep working.
//! - `schema` is **always present**, in column order: the §5 [`type_token`](qfs_core::ColumnType::type_token)
//!   when known, `"unknown"` honestly otherwise.
//! - `meta` carries honest execution fact: `row_count`; `truncated` + the bound that cut
//!   (`limit`/`offset`, the exact vocabulary endpoint paging reuses); `affected` non-null only when
//!   effects ran.
//! - Encodings are schema-discoverable: `timestamp` = epoch-ms UTC; `bytes` = **base64** (the hard
//!   break from the t29 byte-array rendering — re-blessed goldens).
//!
//! A [`PlanPreview`] serializes to the engine's owned [`Preview`](qfs_core::Preview) plus the
//! CLI-level `committed` flag. Any drift fails the golden tests.

use base64::Engine as _;
use qfs_core::{Name, Preview, Row, RowBatch, Schema, Value};
use serde::Serialize;

/// Honest execution metadata carried in the envelope's `meta` (blueprint §14). `row_count` is
/// derived from the rows at serialize time; the rest default to "no bound applied / no effects
/// ran" and are populated where the fact exists (endpoint paging sets `truncated`/`limit`/
/// `offset`, ticket 20260704152639; effect statements set `affected`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResultMeta {
    /// Whether a bound cut the result below the full set.
    pub truncated: bool,
    /// The `limit` that bounded the result, if any (the paging vocabulary).
    pub limit: Option<i64>,
    /// The `offset` the result started at, if any (the paging vocabulary).
    pub offset: Option<i64>,
    /// The affected-row count — non-null only when effects ran (never on a pure read).
    pub affected: Option<i64>,
}

/// An owned set of result rows + their schema — the read-path output the CLI renders. Built
/// from the engine's final [`RowBatch`]; carries the column order so both the JSON object keys
/// and the table columns are deterministic.
#[derive(Debug, Clone, PartialEq)]
pub struct RowSet {
    /// The result schema (column order + names).
    pub schema: Schema,
    /// The result rows, positional to `schema.columns`.
    pub rows: Vec<Row>,
    /// Honest execution metadata (the envelope's `meta`, blueprint §14).
    pub meta: ResultMeta,
}

impl RowSet {
    /// Build a row set from the engine's final batch (no bound applied, no effects ran).
    #[must_use]
    pub fn from_batch(batch: RowBatch) -> Self {
        Self {
            schema: batch.schema,
            rows: batch.rows,
            meta: ResultMeta::default(),
        }
    }

    /// Builder: attach execution metadata (the paging bound / affected count).
    #[must_use]
    pub fn with_meta(mut self, meta: ResultMeta) -> Self {
        self.meta = meta;
        self
    }

    /// The column names, in order.
    #[must_use]
    pub fn columns(&self) -> Vec<&str> {
        self.schema
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect()
    }

    /// The number of result rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the row set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The serialization shape of one row: an ordered map of `column -> value`. Uses an owned
/// `Vec<(Name, &Value)>` serialized as a JSON object so insertion order (= schema order) is
/// preserved deterministically.
struct RowObject<'a> {
    columns: &'a [Name],
    values: &'a [Value],
}

impl Serialize for RowObject<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let n = self.columns.len().min(self.values.len());
        let mut map = serializer.serialize_map(Some(n))?;
        for i in 0..n {
            map.serialize_entry(&self.columns[i], &ValueJson(&self.values[i]))?;
        }
        map.end()
    }
}

/// Serialize a [`Value`] as its **natural** JSON (not serde's externally-tagged enum form): an
/// `Int` becomes a JSON number, `Text` a string, `Null` JSON null, `Timestamp` an epoch-ms
/// number, `Bytes` a **base64** string (blueprint §14 — the hard break from the byte-array
/// rendering), nested `Struct`/`Array`/`Json` recursively. This is the stable agent-facing row
/// shape (`row.subject` is a string, not `{"Text":"…"}`).
struct ValueJson<'a>(&'a Value);

impl Serialize for ValueJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self.0 {
            Value::Null => serializer.serialize_none(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) | Value::Timestamp(i) => serializer.serialize_i64(*i),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::Text(s) => serializer.serialize_str(s),
            Value::Bytes(b) => {
                // base64 (standard alphabet, padded) — schema-discoverable via the `bytes` token.
                serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(b))
            }
            Value::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&ValueJson(item))?;
                }
                seq.end()
            }
            Value::Struct(fields) => {
                let entries: Vec<(&String, &Value)> = fields.iter().collect();
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, &ValueJson(v))?;
                }
                map.end()
            }
            Value::Json(j) => j.serialize(serializer),
            // Value is #[non_exhaustive]: an unmodeled variant falls back to serde's form.
            other => other.serialize(serializer),
        }
    }
}

/// One `{"name","type"}` schema entry: the column name + its §5 type token.
struct SchemaEntry<'a>(&'a qfs_core::Column);

impl Serialize for SchemaEntry<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("SchemaEntry", 2)?;
        s.serialize_field("name", self.0.name.as_str())?;
        s.serialize_field("type", self.0.ty.type_token())?;
        s.end()
    }
}

/// The envelope's `meta` block (blueprint §14). `row_count` is the honest count of serialized
/// rows; the bound fields (`limit`/`offset`/`truncated`) and `affected` are `null` unless a fact
/// exists. Serialized by hand so the field order + null-vs-absent semantics are stable for goldens.
struct MetaJson<'a> {
    row_count: usize,
    meta: &'a ResultMeta,
}

impl Serialize for MetaJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("meta", 5)?;
        s.serialize_field("row_count", &self.row_count)?;
        s.serialize_field("truncated", &self.meta.truncated)?;
        s.serialize_field("limit", &self.meta.limit)?;
        s.serialize_field("offset", &self.meta.offset)?;
        s.serialize_field("affected", &self.meta.affected)?;
        s.end()
    }
}

impl Serialize for RowSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let columns: Vec<Name> = self.schema.columns.iter().map(|c| c.name.clone()).collect();
        let schema: Vec<SchemaEntry> = self.schema.columns.iter().map(SchemaEntry).collect();
        let rows: Vec<RowObject> = self
            .rows
            .iter()
            .map(|r| RowObject {
                columns: &columns,
                values: &r.values,
            })
            .collect();
        // `schema` → `rows` → `meta`, always in this order (blueprint §14).
        let mut s = serializer.serialize_struct("RowSet", 3)?;
        s.serialize_field("schema", &schema)?;
        s.serialize_field("rows", &rows)?;
        s.serialize_field(
            "meta",
            &MetaJson {
                row_count: self.rows.len(),
                meta: &self.meta,
            },
        )?;
        s.end()
    }
}

/// The owned dry-run plan summary the CLI renders for an effect statement: the engine's
/// secret-free [`Preview`] plus whether this was a committed apply (vs a PREVIEW).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanPreview {
    /// The engine preview (effects, per-target affected estimates, irreversible ids).
    pub preview: Preview,
    /// `false` for a PREVIEW (default), `true` when rendered as the result of a `--commit`.
    pub committed: bool,
}

impl PlanPreview {
    /// A preview (not committed).
    #[must_use]
    pub fn preview(preview: Preview) -> Self {
        Self {
            preview,
            committed: false,
        }
    }

    /// A committed-apply summary.
    #[must_use]
    pub fn committed(preview: Preview) -> Self {
        Self {
            preview,
            committed: true,
        }
    }
}
