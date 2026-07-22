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
//! The read executor plans only the **pre-codec** ops (`stmt_without_codec_tail`), so `DECODE`/
//! `ENCODE` and the relational ops that follow them form the **codec tail**, evaluated here over
//! the decoded (data-dependent) schema the planner could not know. A run of trailing relational
//! ops (`WHERE`/`SELECT`/`EXTEND`/`ORDER BY`/`LIMIT`/`DISTINCT`/`AGGREGATE`) runs over the decoded
//! relation through the engine's own lower→partition→`MiniEvaluator` machinery; a **cross-source**
//! stage (`JOIN`/`UNION`/…) after a codec is not yet supported and returns a clear error rather
//! than silently mis-ordering.

use qfs_core::{CodecRegistry, Column, ColumnType, PushdownProfile, Row, RowBatch, Schema, Value};
use qfs_engine::{CombineEngine, MiniEvaluator, ScanResults};
use qfs_parser::{PipeOp, Pipeline, Source, Statement};
use qfs_pushdown::{lower_query, partition_by_source, SourceId, SourceRegistry};

use crate::error::{ErrorKind, ExecError};

/// The well-known column a blob/single-file read carries its raw bytes under (matches the local
/// driver's content column and the write-side `CONTENT_COL`).
const CONTENT_COL: &str = "content";
/// The provenance/join-id column the decode application carries through every decoded row
/// (the source's root-relative address). Owned here, never by a codec.
const PATH_COL: &str = "path";
/// The synthetic source id the codec tail's trailing relational ops route to. `PushdownProfile::
/// None` keeps every op residual, so the engine runs `WHERE`/`SELECT`/`ORDER BY`/`AGGREGATE`/…
/// over the already-decoded batch (the same lower→partition→`MiniEvaluator` machinery the declared
/// view body and the main read path use).
const DECODED_SOURCE: &str = "(decoded)";

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
    let tail = &pipeline.ops[start..];
    let mut current = batch;
    let mut i = 0;
    while i < tail.len() {
        match &tail[i] {
            PipeOp::Decode(c) => {
                current = decode_set(current, &c.fmt, c.relation.as_deref(), &registry)?;
                i += 1;
            }
            PipeOp::Encode(c) => {
                current = encode_rows(&current, &c.fmt, &registry)?;
                i += 1;
            }
            // A run of relational ops after a decode (`WHERE`/`SELECT`/`ORDER BY`/`LIMIT`/
            // `DISTINCT`/`AGGREGATE`/…) evaluates locally over the decoded relation — the decoded
            // schema is late-bound (only known after the decode runs), so these must run here, not
            // in the pushdown plan (blueprint §13b).
            _ => {
                let mut j = i;
                while j < tail.len() && !matches!(tail[j], PipeOp::Decode(_) | PipeOp::Encode(_)) {
                    j += 1;
                }
                current = run_trailing_ops(current, &pipeline.source, &tail[i..j])?;
                i = j;
            }
        }
    }
    Ok(current)
}

/// Evaluate a run of trailing relational ops over the already-decoded `batch`, reusing the
/// engine's lower→partition→[`MiniEvaluator`] machinery (the same the declared view body and the
/// main read path run). The synthetic source keeps every op residual, so the engine executes them
/// over the injected decoded batch. Returns `batch` unchanged for an empty run.
///
/// # Errors
/// [`ExecError`] (usage class) if the ops cannot be lowered/planned over the decoded relation.
fn run_trailing_ops(
    batch: RowBatch,
    source: &Source,
    ops: &[PipeOp],
) -> Result<RowBatch, ExecError> {
    if ops.is_empty() {
        return Ok(batch);
    }
    let schema = batch.schema.clone();
    let source_of = |_: &[String]| SourceId::new(DECODED_SOURCE);
    let schema_of = |_: &SourceId| schema.clone();
    let transform_of = |_: &str| None::<qfs_core::ResolvedTransform>;
    let pipeline = Pipeline {
        source: source.clone(),
        ops: ops.to_vec(),
    };
    let usage = |code: &'static str, msg: String| ExecError::new(ErrorKind::Usage, code, msg);
    let logical = lower_query(&pipeline, &source_of, &schema_of, &transform_of).map_err(|e| {
        usage(
            "codec_then_query",
            format!(
                "a stage after DECODE/ENCODE cannot be evaluated over the decoded relation: {e:?}"
            ),
        )
    })?;
    let mut reg = SourceRegistry::new();
    reg.register(SourceId::new(DECODED_SOURCE), PushdownProfile::None);
    let physical = partition_by_source(&logical, &reg).map_err(|e| {
        usage(
            "codec_then_query",
            format!("could not plan the post-decode stages: {e:?}"),
        )
    })?;
    MiniEvaluator::new()
        .execute(&physical, ScanResults::new(vec![batch]))
        .map_err(|e| {
            usage(
                "codec_then_query",
                format!("post-decode evaluation failed: {e:?}"),
            )
        })
}

/// `DECODE <fmt>[.<relation>]` over a content-bearing batch: decode **each row's** `content` bytes
/// into the named relation (`relation = None` selects the codec's primary relation) and union the
/// per-file relations with schema-widening, carrying the source `path` provenance column onto every
/// decoded row. The single-file case is the one-row instance of this rule.
///
/// Provenance (design brief Ruling 3): each row's `path` value is both carried onto the decoded
/// rows AND threaded to the codec as its `source_path` — a relation whose values normalize against
/// the source's address (the markdown `links` relation's `target_doc`) resolves against the same
/// join id the decoded rows carry, so `links.target_doc` joins `documents.path` for free.
fn decode_set(
    batch: RowBatch,
    fmt: &str,
    relation: Option<&str>,
    reg: &CodecRegistry,
) -> Result<RowBatch, ExecError> {
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
        let path_val = path_idx
            .and_then(|i| row.values.get(i))
            .cloned()
            .unwrap_or(Value::Null);
        // The source's address is the codec's `source_path` provenance (Ruling 3): a relation
        // that normalizes against it (markdown `links.target_doc`) resolves against the very join
        // id the decoded rows carry as `path`.
        let source_path = match &path_val {
            Value::Text(s) => Some(s.as_str()),
            _ => None,
        };
        let decoded = codec
            .decode_relation(relation, &bytes, source_path)
            .map_err(|e| ExecError::from_qfs(&e))?;
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

    /// `SELECT` + `ORDER BY` after a decode run over the decoded relation (blueprint §13b, the
    /// design brief's trailing-relational-op ruling) — the cookbook's collection recipes shape.
    #[test]
    fn select_and_order_by_after_decode_run_over_the_decoded_relation() {
        let stmt = parse_statement(
            "/local/*.json |> decode json |> where n >= 2 |> order by n desc |> select path, n",
        )
        .unwrap();
        let batch = set_batch(&[
            ("/local/a.json", b"{\"n\":1}"),
            ("/local/b.json", b"{\"n\":3}"),
            ("/local/c.json", b"{\"n\":2}"),
        ]);
        let out = apply_codecs(batch, &stmt).unwrap();
        // n>=2 keeps b(3) and c(2); ordered desc → b then c; projected to (path, n).
        assert_eq!(out.rows.len(), 2);
        assert_eq!(out.schema.columns.len(), 2);
        assert_eq!(out.schema.columns[0].name.as_str(), "path");
        assert!(matches!(&out.rows[0].values[0], Value::Text(s) if s == "/local/b.json"));
        assert!(matches!(&out.rows[1].values[0], Value::Text(s) if s == "/local/c.json"));
    }

    /// `decode md.documents` over a collected `*.md` set (design brief Ruling 1 + the grammar
    /// suffix): the relation-qualified token routes to the codec's `documents` relation — one row
    /// per file with `title` derived (frontmatter, else first heading) — each carrying its `path`
    /// provenance. The bare `path` column is the join id `links.target_doc` resolves against.
    #[test]
    fn decode_md_documents_relation_over_a_set() {
        let stmt = parse_statement("/local/*.md |> decode md.documents").unwrap();
        let batch = set_batch(&[
            (
                "notes/first.md",
                b"# First note\n\nback to [plan](../plan.md)\n",
            ),
            (
                "plan.md",
                "---\ntitle: The Plan\n---\n\n# H1\n\n## H2\n\n[the note](notes/first.md)\n"
                    .as_bytes(),
            ),
        ]);
        let out = apply_codecs(batch, &stmt).unwrap();
        assert_eq!(out.rows.len(), 2, "one documents row per file");
        assert_eq!(out.schema.columns[0].name.as_str(), "path");
        let path = |i: usize| match &out.rows[i].values[0] {
            Value::Text(s) => s.as_str(),
            _ => "",
        };
        assert_eq!(path(0), "notes/first.md");
        assert_eq!(path(1), "plan.md");
        let title_idx = out
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "title")
            .expect("title column");
        // `title`: notes/first.md → first ATX heading; plan.md → frontmatter title.
        assert!(matches!(&out.rows[0].values[title_idx], Value::Text(s) if s == "First note"));
        assert!(matches!(&out.rows[1].values[title_idx], Value::Text(s) if s == "The Plan"));
    }

    /// `decode md.links` over a collected `*.md` set: the `links` relation yields zero-or-more rows
    /// per file, each carrying the full nested `source_section_path`, and `target_doc` normalized
    /// against the source's `path` provenance so it JOINS `documents.path` (Ruling 3). Proves the
    /// per-file cardinality difference rides the same per-row-then-union machinery.
    #[test]
    fn decode_md_links_relation_normalizes_target_doc_against_provenance() {
        let stmt = parse_statement("/local/*.md |> decode md.links").unwrap();
        let batch = set_batch(&[(
            "notes/first.md",
            b"# First note\n\n## Detail\n\nsee [plan](../plan.md) and [ext](https://x.example/y)\n",
        )]);
        let out = apply_codecs(batch, &stmt).unwrap();
        // Two links in one file; each row carries the source's provenance path.
        assert_eq!(out.rows.len(), 2);
        assert_eq!(out.schema.columns[0].name.as_str(), "path");
        let col = |name: &str| {
            out.schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == name)
                .unwrap_or_else(|| panic!("column {name}"))
        };
        let sec = col("source_section_path");
        let tgt = col("target");
        let tdoc = col("target_doc");
        // The in-tree link: nested heading path in order, target_doc joinable to documents.path.
        let section: Vec<String> = match &out.rows[0].values[sec] {
            Value::Array(items) => items
                .iter()
                .filter_map(|v| match v {
                    Value::Text(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            other => panic!("source_section_path should be an Array, got {other:?}"),
        };
        assert_eq!(section, vec!["First note", "Detail"]);
        // `../plan.md` from `notes/first.md` normalizes to the root-relative `plan.md` join id.
        assert!(matches!(&out.rows[0].values[tdoc], Value::Text(s) if s == "plan.md"));
        // The external link keeps its target and is not joinable.
        assert!(matches!(&out.rows[1].values[tgt], Value::Text(s) if s == "https://x.example/y"));
        assert!(matches!(&out.rows[1].values[tdoc], Value::Null));
    }

    /// A relation qualifier over a single-relation codec is a clear usage error, not a silent
    /// wrong decode (design brief Ruling 1: `decode json.nope`).
    #[test]
    fn relation_on_a_single_relation_codec_is_an_error() {
        let stmt = parse_statement("/local/a.json |> decode json.nope").unwrap();
        let err = apply_codecs(set_batch(&[("a.json", b"{\"k\":1}")]), &stmt).unwrap_err();
        assert!(
            err.to_string().contains("nope") || err.to_string().contains("relation"),
            "a bad relation names the offending relation: {err}"
        );
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
