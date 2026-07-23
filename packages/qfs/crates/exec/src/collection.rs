//! blueprint §13b — the **declared collection-set registration read** (mission
//! `a-file-collection-is-a-declared-set-over-any-blob-source`, ticket 20260722090300).
//!
//! A collection is a **declared, named set registered over other paths** — a stored view created
//! through the ordinary definition layer with zero new grammar (`CREATE VIEW <name> AS
//! /local/<root>/**/*.md |> decode md.<relation>` desugars to an `INSERT` into a registry path,
//! blueprint §3). Reading a registered set is *executing its stored pipeline*: the collect segment
//! scans the blob source (materializing each file's `content`), and the `DECODE <fmt>.<relation>`
//! codec tail turns the collected bytes into the named relation's rows.
//!
//! ## The one registration-layer step: the root-relative `path` join id (design brief Ruling 3)
//! A `/local` listing carries the **VFS path** (`/local/notes/a.md`) as its `path` provenance
//! column; the compiled `/markdown` driver emits the **root-relative** join id (`notes/a.md`). The
//! *registration/codec layer* produces the root-relative form by stripping the collection-root
//! prefix — the static (pre-glob) head of the stored body's source path. Stripping happens
//! **before** the decode, so the codec normalizes `links.target_doc` against the same root-relative
//! anchor the compiled driver used, and `documents.path` / `links.source_doc` carry the same join
//! id. That is exactly what makes the declared `documents`/`links` **row-equivalent** to the
//! compiled driver (the §13 twin-and-retire ratchet aimed inward).
//!
//! Raw `decode md.documents` over a bare `/local` set keeps the VFS `path` (the collect segment's
//! canonical id); only the *registration* strips the mount+root prefix — so the generic per-row
//! decode's provenance is unchanged (Ruling 3 fixes the strip to the registration layer, not the
//! codec application).

use qfs_core::{Row, RowBatch, Schema, Value};
use qfs_parser::{Source, Statement};

use crate::codec::apply_codecs;
use crate::error::ExecError;

/// The provenance/join-id column every decoded collection row carries (the source's address).
pub const PATH_COL: &str = "path";

/// The **collection root** a registered set is declared over: the static (pre-glob) head of a
/// pipeline source path. `/local/docs/**/*.md` → `/local/docs`; `/local/*.md` → `/local`. Returns
/// `None` for a non-path source, or when the very first segment is already a glob (no static root
/// to strip).
#[must_use]
pub fn collection_root(source: &Source) -> Option<String> {
    let Source::Path(path) = source else {
        return None;
    };
    let mut root = String::new();
    for seg in &path.segments {
        if seg.glob {
            break;
        }
        root.push('/');
        root.push_str(&seg.name);
    }
    if root.is_empty() {
        None
    } else {
        Some(root)
    }
}

/// Rewrite the provenance `path` column of `batch` to the form **root-relative** to `root` (strip
/// the leading `root` + `/`). A row whose `path` is not under `root` is left unchanged (robust — a
/// mixed listing never fails). A batch with no `path` column is returned unchanged. This is the
/// registration layer's mount+root-prefix strip (design brief Ruling 3), applied to the scanned
/// listing **before** the decode so every decoded row's join id is root-relative.
#[must_use]
pub fn to_root_relative(batch: RowBatch, root: &str) -> RowBatch {
    let Some(path_idx) = batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == PATH_COL)
    else {
        return batch;
    };
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let rows: Vec<Row> = batch
        .rows
        .into_iter()
        .map(|mut r| {
            if let Some(Value::Text(p)) = r.values.get(path_idx) {
                if let Some(rel) = p.strip_prefix(&prefix) {
                    r.values[path_idx] = Value::Text(rel.to_string());
                }
            }
            r
        })
        .collect();
    RowBatch::new(batch.schema, rows)
}

/// Read a **registered collection set** (blueprint §13b): given the materialized listing the stored
/// body's collect segment scanned (`path` + `content` per file) and the stored body statement, strip
/// the collection-root prefix from `path` to the root-relative join id, then run the body's `DECODE
/// <fmt>.<relation>` tail over the stripped batch. The decoded rows are the registered set's rows —
/// row-equivalent to the compiled driver over the same files.
///
/// `scanned` is supplied by the caller (the binary's `/local` read facet, materialized): `qfs-exec`
/// stays off the concrete blob drivers, exactly as the declared-view read injects its wire fetch.
///
/// # Errors
/// [`ExecError`] if the stored body is not a codec-tail read query, or the decode fails.
pub fn read_registered_collection(
    scanned: RowBatch,
    body: &Statement,
) -> Result<RowBatch, ExecError> {
    let Statement::Query(pipeline) = body else {
        return Err(ExecError::usage(
            "a registered collection body must be a read query (a collect + DECODE pipeline)",
        ));
    };
    let stripped = match collection_root(&pipeline.source) {
        Some(root) => to_root_relative(scanned, &root),
        None => scanned,
    };
    apply_codecs(stripped, body)
}

/// The declared **schema** a registered collection view reports through `DESCRIBE` — the codec
/// relation's canonical schema (`documents`: `path`, `title`, `frontmatter`; `links`: `source_doc`,
/// `source_section_path`, `target`, `target_doc`, `line`). Kept as a thin re-export so the viewer
/// and agents discover the registered set's shape generically, identical to what the compiled
/// `/markdown` driver's `DESCRIBE` reported. Only the markdown codec declares named relations today.
#[must_use]
pub fn markdown_relation_describe_schema(relation: qfs_core::MarkdownRelation) -> Schema {
    // The single source of truth for the relation schemas lives in the codec (qfs-core re-exports
    // it), so the registered view's DESCRIBE and the decode agree by construction.
    qfs_core::markdown_relation_schema(relation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_core::{Column, ColumnType, Value};
    use qfs_parser::parse_statement;

    fn source_of(q: &str) -> Source {
        match parse_statement(q).unwrap() {
            Statement::Query(p) => p.source,
            _ => panic!("expected a query"),
        }
    }

    #[test]
    fn collection_root_is_the_static_pre_glob_head() {
        assert_eq!(
            collection_root(&source_of("/local/docs/**/*.md |> decode md.documents")).as_deref(),
            Some("/local/docs")
        );
        assert_eq!(
            collection_root(&source_of("/local/*.md |> decode md.links")).as_deref(),
            Some("/local")
        );
        // A first-segment glob has no static root.
        assert_eq!(collection_root(&source_of("/*/x.md |> decode md")), None);
    }

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
    fn to_root_relative_strips_the_collection_root_prefix() {
        let batch = set_batch(&[
            ("/local/docs/plan.md", b"x"),
            ("/local/docs/notes/first.md", b"y"),
        ]);
        let out = to_root_relative(batch, "/local/docs");
        let paths: Vec<&str> = out
            .rows
            .iter()
            .filter_map(|r| match &r.values[0] {
                Value::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec!["plan.md", "notes/first.md"]);
    }

    #[test]
    fn registered_documents_carry_the_root_relative_join_id() {
        // The registration read strips `/local` and runs `decode md.documents`, so the row's `path`
        // join id is root-relative (the compiled driver's form) and `title` derives from the first
        // ATX heading — the registration-level shape the equivalence gate pins.
        let scanned = set_batch(&[(
            "/local/notes/first.md",
            b"# First note\n\n[p](../plan.md)\n",
        )]);
        let body = parse_statement("/local/**/*.md |> decode md.documents").unwrap();
        let out = read_registered_collection(scanned, &body).unwrap();
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.schema.columns[0].name.as_str(), "path");
        assert!(matches!(&out.rows[0].values[0], Value::Text(s) if s == "notes/first.md"));
        let title = out
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "title")
            .expect("title column");
        assert!(matches!(&out.rows[0].values[title], Value::Text(s) if s == "First note"));
    }

    #[test]
    fn registered_links_normalize_target_doc_against_the_root_relative_source() {
        // With the root stripped BEFORE decode, `target_doc` normalizes against the root-relative
        // `notes/first.md` — so `../plan.md` resolves to the join id `plan.md`, exactly as the
        // compiled driver emits (design brief Ruling 3).
        let scanned = set_batch(&[(
            "/local/notes/first.md",
            b"# First note\n\n## Detail\n\nsee [plan](../plan.md)\n",
        )]);
        let body = parse_statement("/local/**/*.md |> decode md.links").unwrap();
        let out = read_registered_collection(scanned, &body).unwrap();
        assert_eq!(out.rows.len(), 1);
        let tdoc = out
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "target_doc")
            .expect("target_doc column");
        assert!(matches!(&out.rows[0].values[tdoc], Value::Text(s) if s == "plan.md"));
        let sdoc = out
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == "source_doc")
            .expect("source_doc column");
        assert!(matches!(&out.rows[0].values[sdoc], Value::Text(s) if s == "notes/first.md"));
    }
}
