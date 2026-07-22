//! Codec execution for the read path (blueprint §4/§13b): apply the `DECODE`/`ENCODE <fmt>`
//! pipe stages — and the relational ops that trail them — that the pushdown lowering
//! deliberately stops at.
//!
//! Codecs are schema-shaping, **local-only** transforms (never pushed to a driver), so they run
//! after the residual [`MiniEvaluator`](qfs_engine::MiniEvaluator) has produced the final batch.
//! The chain runs in pipeline order: `decode` turns a blob source's `content` bytes into rows
//! (via the registry codec's pure `bytes -> RowBatch`); `encode` collapses rows back into encoded
//! bytes (`RowBatch -> bytes`).
//!
//! ## Per-row decode over a collected set (blueprint §13b)
//! `DECODE` applies the codec's `bytes -> rows` contract to **each row** of a multi-row
//! content-bearing batch (a `/local/*.md` glob, a directory), and the per-file relations
//! **union** with schema-widening (a column absent from one file's relation reads as null). The
//! single-file case is the one-row instance of the same rule — there is no single-blob refusal.
//! Every decoded row carries the source's **`path`** provenance column (the canonical join id),
//! owned by this decode application, not by any codec.
//!
//! ## The codec tail
//! Lowering stops at the first codec (`qfs_pushdown::lower_query`), so `DECODE`/`ENCODE` and the
//! relational ops that follow them form the **codec tail**, evaluated here over the decoded
//! (data-dependent) schema the planner could not know. `WHERE`/`LIMIT` after a decode run locally
//! over the decoded batch; a trailing op the local tail cannot yet run returns a clear error
//! rather than silently mis-ordering.

use qfs_core::{CodecRegistry, Column, ColumnType, Row, RowBatch, Schema, Value};
use qfs_engine::apply_residual;
use qfs_parser::{PipeOp, Statement};
use qfs_pushdown::lower_predicate;

use crate::error::{ErrorKind, ExecError};

/// The well-known column a blob/single-file read carries its raw bytes under (matches the local
/// driver's content column and the write-side `CONTENT_COL`).
const CONTENT_COL: &str = "content";
/// The provenance/join-id column the decode application carries through every decoded row
/// (the source's root-relative address). Owned here, never by a codec.
const PATH_COL: &str = "path";

/// Whether `stmt` carries a `DECODE` stage — the signal the read executor uses to ask a blob
/// scan to materialize each row's `content` bytes (blueprint §13b, plan-driven materialization).
pub(crate) fn has_decode_stage(stmt: &Statement) -> bool {
    let Statement::Query(pipeline) = stmt else {
        return false;
    };
    pipeline
        .ops
        .iter()
        .any(|op| matches!(op, PipeOp::Decode(_)))
}

/// The statement to build the pushdown plan from: `stmt` truncated at the first codec stage, so
/// the plan covers only the **pre-codec** ops (blueprint §13b). The codec stages and every op
/// after them are the codec tail [`apply_codecs`] runs locally over the scanned batch — they must
/// not be lowered into a scan residual over a schema the decode has not yet produced. Returns
/// `None` when the statement carries no codec (plan the original statement unchanged).
pub(crate) fn stmt_without_codec_tail(stmt: &Statement) -> Option<Statement> {
    let Statement::Query(pipeline) = stmt else {
        return None;
    };
    let start = pipeline
        .ops
        .iter()
        .position(|op| matches!(op, PipeOp::Decode(_) | PipeOp::Encode(_)))?;
    Some(Statement::Query(qfs_parser::Pipeline {
        source: pipeline.source.clone(),
        ops: pipeline.ops[..start].to_vec(),
    }))
}

/// Apply the statement's codec tail (`DECODE`/`ENCODE` + any trailing relational ops) to the
/// executed `batch`, in pipeline order. Returns `batch` unchanged when the pipeline carries no
/// codec stage (the common case — zero cost, no registry built).
///
/// # Errors
/// [`ExecError`] (usage class) if a codec is unknown, a `DECODE` has no blob to read, the bytes
/// are not valid for the format, or a trailing op cannot be evaluated locally.
pub(crate) fn apply_codecs(batch: RowBatch, stmt: &Statement) -> Result<RowBatch, ExecError> {
    let Statement::Query(pipeline) = stmt else {
        return Ok(batch);
    };
    // The codec tail begins at the first DECODE/ENCODE; everything before it was already run by
    // the pushdown plan + engine residual (lowering stops at the first codec).
    let Some(start) = pipeline
        .ops
        .iter()
        .position(|op| matches!(op, PipeOp::Decode(_) | PipeOp::Encode(_)))
    else {
        return Ok(batch);
    };
    // The six shipped codecs are builtins; resolve through a fresh builtin registry. A
    // backend-registered custom codec would thread the engine's CodecRegistry here instead.
    let registry = CodecRegistry::with_builtins();
    let mut current = batch;
    for op in &pipeline.ops[start..] {
        current = apply_tail_op(current, op, &registry)?;
    }
    Ok(current)
}

/// Run one codec-tail op over `batch`.
fn apply_tail_op(batch: RowBatch, op: &PipeOp, reg: &CodecRegistry) -> Result<RowBatch, ExecError> {
    match op {
        PipeOp::Decode(c) => decode_set(batch, &c.fmt, reg),
        PipeOp::Encode(c) => encode_rows(&batch, &c.fmt, reg),
        // A `WHERE` after a decode filters the decoded relation locally (the decoded schema is
        // late-bound — only known after the decode runs; blueprint §13b Requirement 3).
        PipeOp::Where(e) => {
            let predicate = lower_predicate(e).map_err(|err| {
                ExecError::new(
                    ErrorKind::Usage,
                    "codec_where_unsupported",
                    format!("this WHERE cannot be evaluated after a decode: {err:?}"),
                )
            })?;
            Ok(apply_residual(batch, &predicate))
        }
        // A `LIMIT` after a decode truncates the decoded relation.
        PipeOp::Limit(n) => {
            let keep = (*n).max(0) as usize;
            let mut b = batch;
            b.rows.truncate(keep);
            Ok(b)
        }
        // Schema-neutral / effect-adjacent ops are positionally harmless (they neither reorder
        // nor reshape the decoded relation in a way the tail must model).
        PipeOp::Extend(_) | PipeOp::Set(_) | PipeOp::As(_) | PipeOp::Call(_) => Ok(batch),
        // Any other relational op after a codec is not yet evaluable locally over the decoded
        // relation — a clear, named error beats silently mis-ordering.
        other => Err(ExecError::new(
            ErrorKind::Usage,
            "codec_then_query",
            format!(
                "the `{}` stage after DECODE/ENCODE is not yet supported — only WHERE/LIMIT run \
                 after a decode today",
                op_label(other)
            ),
        )),
    }
}

/// `DECODE <fmt>` over a content-bearing batch: decode **each row's** `content` bytes and union
/// the per-file relations with schema-widening, carrying the source `path` provenance column onto
/// every decoded row. The single-file case is the one-row instance of this rule.
fn decode_set(batch: RowBatch, fmt: &str, reg: &CodecRegistry) -> Result<RowBatch, ExecError> {
    let codec = reg.resolve(fmt).map_err(|e| ExecError::from_qfs(&e))?;
    let content_idx = batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == CONTENT_COL)
        .ok_or_else(|| {
            ExecError::new(
                ErrorKind::Usage,
                "decode_needs_blob",
                "DECODE expects a blob source with a `content` column (e.g. a /local file set)"
                    .to_string(),
            )
        })?;
    let path_idx = batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == PATH_COL);

    // Ordered union of the decoded relation's columns (first-seen order), excluding `path` (the
    // provenance column the application owns), plus each output row's `(path, decoded values)`.
    let mut col_names: Vec<String> = Vec::new();
    let mut col_types: Vec<ColumnType> = Vec::new();
    let mut out_rows: Vec<(Value, Vec<(usize, Value)>)> = Vec::new();

    for row in &batch.rows {
        let bytes = match row.values.get(content_idx) {
            Some(Value::Bytes(b)) => b.clone(),
            Some(Value::Text(s)) => s.clone().into_bytes(),
            // A content-free row (a directory entry caught by a glob, or an unmaterialized
            // listing) contributes no decoded rows — robust, never an error.
            Some(Value::Null) | None => continue,
            _ => {
                return Err(ExecError::new(
                    ErrorKind::Usage,
                    "decode_needs_blob",
                    "the `content` column is not bytes".to_string(),
                ))
            }
        };
        let decoded = codec.decode(&bytes).map_err(|e| ExecError::from_qfs(&e))?;
        let path_val = path_idx
            .and_then(|i| row.values.get(i))
            .cloned()
            .unwrap_or(Value::Null);
        for srow in &decoded.rows {
            let mut cells: Vec<(usize, Value)> = Vec::new();
            for (col, val) in decoded.schema.columns.iter().zip(&srow.values) {
                if col.name.as_str() == PATH_COL {
                    continue; // the provenance `path` wins over any decoded `path`
                }
                let idx = match col_names.iter().position(|n| n == col.name.as_str()) {
                    Some(i) => i,
                    None => {
                        col_names.push(col.name.clone());
                        col_types.push(col.ty.clone());
                        col_names.len() - 1
                    }
                };
                cells.push((idx, val.clone()));
            }
            out_rows.push((path_val.clone(), cells));
        }
    }

    // Assemble the widened schema: [path?] + the union columns (all nullable — a column may be
    // absent from some files' relations, reading as null there).
    let has_path = path_idx.is_some();
    let mut columns: Vec<Column> = Vec::new();
    if has_path {
        columns.push(Column::new(PATH_COL, ColumnType::Text, false));
    }
    for (name, ty) in col_names.iter().zip(&col_types) {
        columns.push(Column::new(name.clone(), ty.clone(), true));
    }
    let schema = Schema::new(columns);

    let width = col_names.len();
    let rows: Vec<Row> = out_rows
        .into_iter()
        .map(|(path_val, cells)| {
            let mut values: Vec<Value> = Vec::with_capacity(width + usize::from(has_path));
            if has_path {
                values.push(path_val);
            }
            let mut slots = vec![Value::Null; width];
            for (idx, val) in cells {
                slots[idx] = val;
            }
            values.extend(slots);
            Row::new(values)
        })
        .collect();

    Ok(RowBatch::new(schema, rows))
}

/// `ENCODE <fmt>` — rows into encoded bytes. The provenance `path` column (added by a prior
/// decode, owned by the application) is dropped before encoding: it is not the codec's data.
fn encode_rows(batch: &RowBatch, fmt: &str, reg: &CodecRegistry) -> Result<RowBatch, ExecError> {
    let codec = reg.resolve(fmt).map_err(|e| ExecError::from_qfs(&e))?;
    let to_encode = drop_provenance(batch);
    let bytes = codec
        .encode(&to_encode)
        .map_err(|e| ExecError::from_qfs(&e))?;
    Ok(encoded_batch(bytes))
}

/// Return `batch` without the provenance `path` column, if present (the decode application's
/// column, not the codec's data). A batch without one is returned as a cheap clone.
fn drop_provenance(batch: &RowBatch) -> RowBatch {
    let Some(path_idx) = batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == PATH_COL)
    else {
        return batch.clone();
    };
    let columns: Vec<Column> = batch
        .schema
        .columns
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != path_idx)
        .map(|(_, c)| c.clone())
        .collect();
    let rows: Vec<Row> = batch
        .rows
        .iter()
        .map(|r| {
            Row::new(
                r.values
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != path_idx)
                    .map(|(_, v)| v.clone())
                    .collect(),
            )
        })
        .collect();
    RowBatch::new(Schema::new(columns), rows)
}

/// Wrap encoded bytes as a single-row, single-column (`content`) text batch — the form the
/// renderer prints. The six builtin formats are all UTF-8 text, so a lossy decode is safe.
fn encoded_batch(bytes: Vec<u8>) -> RowBatch {
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let schema = Schema::new(vec![Column::new(CONTENT_COL, ColumnType::Text, false)]);
    RowBatch::new(schema, vec![Row::new(vec![Value::Text(text)])])
}

/// A short label for a pipe op, for the `codec_then_query` diagnostic.
fn op_label(op: &PipeOp) -> &'static str {
    match op {
        PipeOp::Where(_) => "WHERE",
        PipeOp::Select(_) => "SELECT",
        PipeOp::Extend(_) => "EXTEND",
        PipeOp::Set(_) => "SET",
        PipeOp::Aggregate(_) => "AGGREGATE",
        PipeOp::GroupBy(_) => "GROUP BY",
        PipeOp::OrderBy(_) => "ORDER BY",
        PipeOp::Limit(_) => "LIMIT",
        PipeOp::Distinct => "DISTINCT",
        PipeOp::Join(_) => "JOIN",
        PipeOp::Union(_) => "UNION",
        PipeOp::Except(_) => "EXCEPT",
        PipeOp::Intersect(_) => "INTERSECT",
        PipeOp::Expand(_) => "EXPAND",
        _ => "this",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_parser::parse_statement;

    /// A single-file `/local` read batch: the listing columns plus a `content` Bytes column.
    fn blob_batch(bytes: &[u8]) -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("name", ColumnType::Text, false),
            Column::new("content", ColumnType::Bytes, true),
        ]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("config.json".to_string()),
                Value::Bytes(bytes.to_vec()),
            ])],
        )
    }

    /// A multi-file collected set: `path` + `content` per row (the /local glob shape after
    /// plan-driven materialization).
    fn set_batch(files: &[(&str, &[u8])]) -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("path", ColumnType::Text, false),
            Column::new("content", ColumnType::Bytes, true),
        ]);
        let rows = files
            .iter()
            .map(|(p, b)| {
                Row::new(vec![
                    Value::Text((*p).to_string()),
                    Value::Bytes(b.to_vec()),
                ])
            })
            .collect();
        RowBatch::new(schema, rows)
    }

    #[test]
    fn decode_json_then_encode_yaml_transcodes() {
        let stmt = parse_statement("/local/config.json |> decode json |> encode yaml").unwrap();
        let out = apply_codecs(blob_batch(b"{\"k\":1}\n"), &stmt).unwrap();
        assert_eq!(out.schema.columns.len(), 1);
        assert_eq!(out.schema.columns[0].name.as_str(), "content");
        let Value::Text(yaml) = &out.rows[0].values[0] else {
            panic!("expected text yaml");
        };
        assert!(
            yaml.contains("k: 1"),
            "yaml encodes the decoded object: {yaml:?}"
        );
    }

    #[test]
    fn decode_alone_unpacks_rows() {
        let stmt = parse_statement("/local/config.json |> decode json").unwrap();
        let out = apply_codecs(blob_batch(b"{\"k\":1}\n"), &stmt).unwrap();
        assert!(out.schema.columns.iter().any(|c| c.name.as_str() == "k"));
    }

    #[test]
    fn no_codec_stages_pass_through_unchanged() {
        let stmt = parse_statement("/local/config.json |> select name").unwrap();
        let batch = blob_batch(b"{\"k\":1}\n");
        let out = apply_codecs(batch.clone(), &stmt).unwrap();
        assert_eq!(out.schema.columns.len(), batch.schema.columns.len());
    }

    #[test]
    fn unknown_codec_is_a_usage_error() {
        let stmt = parse_statement("/local/config.json |> decode nope").unwrap();
        let err = apply_codecs(blob_batch(b"{}"), &stmt).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Capability);
    }

    /// Per-row decode over a collected set (blueprint §13b): a `*.json` glob decodes to one row
    /// per file, each carrying its `path` provenance, unioned schema-widening.
    #[test]
    fn decode_set_is_per_row_with_provenance() {
        let stmt = parse_statement("/local/*.json |> decode json").unwrap();
        let batch = set_batch(&[
            ("/local/a.json", b"{\"k\":1,\"only_a\":true}"),
            ("/local/b.json", b"{\"k\":2}"),
        ]);
        let out = apply_codecs(batch, &stmt).unwrap();
        assert_eq!(out.rows.len(), 2, "one decoded row per file");
        // Provenance path is the first column and carries each file's path.
        assert_eq!(out.schema.columns[0].name.as_str(), "path");
        let paths: Vec<&str> = out
            .rows
            .iter()
            .filter_map(|r| match &r.values[0] {
                Value::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec!["/local/a.json", "/local/b.json"]);
        // Schema-widening: `only_a` exists; b's row reads null there, not an error.
        let only_a = out
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "only_a")
            .expect("only_a column present");
        assert!(matches!(out.rows[1].values[only_a], Value::Null));
    }

    /// A WHERE after a decode filters the decoded relation; a file missing the key reads as null
    /// (blueprint §13b Requirement 3) and is filtered out, not an error.
    #[test]
    fn where_after_decode_filters_the_decoded_relation() {
        let stmt = parse_statement("/local/*.json |> decode json |> where k == 2").unwrap();
        let batch = set_batch(&[
            ("/local/a.json", b"{\"k\":1}"),
            ("/local/b.json", b"{\"k\":2}"),
            ("/local/c.json", b"{\"other\":9}"),
        ]);
        let out = apply_codecs(batch, &stmt).unwrap();
        assert_eq!(out.rows.len(), 1, "only b.json matches k == 2");
        assert!(matches!(&out.rows[0].values[0], Value::Text(s) if s == "/local/b.json"));
    }

    /// The single-file case passes as the one-row instance of the per-row rule (no single-blob
    /// refusal): a lone content row decodes, and a multi-row set no longer errors.
    #[test]
    fn single_file_is_the_one_row_instance() {
        let stmt = parse_statement("/local/a.json |> decode json").unwrap();
        let out = apply_codecs(set_batch(&[("/local/a.json", b"{\"k\":7}")]), &stmt).unwrap();
        assert_eq!(out.rows.len(), 1);
        let k = out
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "k")
            .expect("k column");
        assert!(matches!(out.rows[0].values[k], Value::Int(7)));
    }
}
