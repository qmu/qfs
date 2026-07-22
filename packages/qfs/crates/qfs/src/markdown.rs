//! The `/markdown` collection-path composition root: the `path_binding` loader + the on-disk
//! tree walk behind the async [`MarkdownReadDriver`] read facet, both hosted in the **`qfs`
//! binary crate** (mission `markdown-trees-are-queryable-as-documents-and-links-tables`).
//!
//! ## Why the walk lives in the binary (not the driver crate)
//! `qfs-driver-markdown` is the pure introspective + parser crate (no I/O, wasm-buildable); the
//! binary IS the leaf and the ONE place that opens a real path (decision F), so the `std::fs`
//! tree walk dead-ends here — exactly the `/claude` (`DirSessionSource`) and `/sys` split. The
//! walk feeds each file's root-relative path + text into the crate's pure
//! [`qfs_driver_markdown::parse_document`].
//!
//! ## Roots come from declarations only (the declared-drivers convention)
//! A root is declared by `qfs connect /markdown/<name> --driver markdown --at '<root-dir>'`
//! (or the language `CONNECT … TO markdown AT '…'`) — a committed, ledgered `path_binding` row
//! (see the mission's design brief). **There is no `QFS_MARKDOWN_*` env var**, deliberately:
//! the deprecated env-var seam is not extended to new drivers. With no binding, nothing is
//! wired — fail-closed, like every mount.
//!
//! ## Rescan = read-through (the design-brief ruling)
//! The read facet is **stateless**: every scan re-walks and re-parses the declared root, so
//! `documents`/`links` can never be stale — after files change, the very next query reflects
//! them (pinned by [`tests::rescan_via_read_through_reflects_tree_changes`]). If a later slice
//! adds an index/cache, an explicit `CALL markdown.rescan` becomes the entry point.
//!
//! ## Walk scope (documented)
//! Recursive under the declared root; only `*.md` files; dot-entries (files and directories)
//! skipped; symlinks not followed (no cycle risk); deterministic path order.

use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use qfs_core::{CfsError, RowBatch};
use qfs_driver_markdown::{
    documents_schema, links_schema, parse_document, tree_and_node_for_path, MarkdownDriver,
    MarkdownNode,
};
use qfs_exec::ReadDriver;
use qfs_pushdown::ScanNode;

/// The declared markdown roots from the System-DB `path_binding` registry (the canonical
/// source, mirroring `git.rs`'s `path_binding_git_connections`): each FULL-connect binding
/// whose `driver_id` is `markdown`, as `name → at_locator`. `name` is the segment after
/// `/markdown/`, so a `CONNECT /markdown/docs TO markdown AT '<dir>'` binding scans under
/// `/markdown/docs/...`. A markdown root carries no secret. Empty when no System DB / no
/// binding resolves (best-effort, never panics — an unreadable root fails closed at scan time).
fn path_binding_markdown_connections() -> BTreeMap<String, String> {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return BTreeMap::new();
    };
    let conn = sys.into_db().into_connection();
    crate::path_binding::db_list_bindings(&conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.alias_of.is_none())
        .filter_map(|b| {
            if b.driver_id.as_deref() != Some("markdown") {
                return None;
            }
            let name = b
                .path
                .strip_prefix("/markdown/")?
                .split('/')
                .next()
                .filter(|s| !s.is_empty())?
                .to_string();
            let at = b.at_locator.clone()?;
            Some((name, at))
        })
        .collect()
}

/// Whether any markdown root is declared — the registration gate `shell.rs` consults. With no
/// binding the mount and read facet are simply not wired (fail-closed).
#[must_use]
pub fn has_connections() -> bool {
    !path_binding_markdown_connections().is_empty()
}

/// The pure planning/describe driver instance (rootless — describe is static; the roots feed
/// only the read facet).
#[must_use]
pub fn markdown_driver() -> MarkdownDriver {
    MarkdownDriver::new()
}

/// The async read facet (the `/markdown` counterpart of `claude.rs`'s `ClaudeReadDriver`):
/// resolves a `/markdown/<name>/<table>` scan to the declared root and re-scans the tree.
/// Lives in the binary because `ReadDriver` is a qfs-exec type and the driver crate stays off
/// qfs-exec (dep direction).
pub struct MarkdownReadDriver {
    roots: BTreeMap<String, PathBuf>,
}

impl MarkdownReadDriver {
    /// Build the read facet over the persisted `path_binding` declarations.
    #[must_use]
    pub fn open_default() -> Self {
        Self {
            roots: path_binding_markdown_connections()
                .into_iter()
                .map(|(name, at)| (name, PathBuf::from(at)))
                .collect(),
        }
    }

    /// Build the read facet over explicit `(name, root)` pairs (the test + composition seam).
    #[must_use]
    pub fn with_roots(roots: impl IntoIterator<Item = (String, PathBuf)>) -> Self {
        Self {
            roots: roots.into_iter().collect(),
        }
    }

    /// Scan one declared tree into its `documents` + `links` batches. Read-through: this
    /// re-walks and re-parses on every call (the design-brief rescan ruling).
    fn scan_tree(&self, name: &str, node: MarkdownNode) -> Result<RowBatch, String> {
        let root = self
            .roots
            .get(name)
            .ok_or_else(|| format!("no markdown root declared for `{name}`"))?;
        let mut files: Vec<PathBuf> = Vec::new();
        collect_md_files(root, &mut files);
        files.sort();
        let mut documents = Vec::new();
        let mut links = Vec::new();
        for file in files {
            let Ok(rel) = file.strip_prefix(root) else {
                continue;
            };
            let rel = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            // A file that vanishes or is not UTF-8 between listing and reading is skipped —
            // the surviving tree still lists (robust, never a panic).
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let parsed = parse_document(&rel, &text);
            links.extend(parsed.link_rows());
            documents.push(parsed.document_row());
        }
        Ok(match node {
            MarkdownNode::Documents => RowBatch::new(documents_schema(), documents),
            MarkdownNode::Links => RowBatch::new(links_schema(), links),
        })
    }
}

/// Recursively collect `*.md` files under `dir`: dot-entries skipped, symlinks not followed.
/// A missing/unreadable directory contributes nothing (fail-closed read, never an error).
fn collect_md_files(dir: &FsPath, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        // `file_type()` does NOT traverse symlinks — a symlinked dir/file is skipped entirely.
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            collect_md_files(&path, out);
        } else if ft.is_file() && name.to_ascii_lowercase().ends_with(".md") {
            out.push(path);
        }
    }
}

#[async_trait::async_trait]
impl ReadDriver for MarkdownReadDriver {
    async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError> {
        let (name, node) =
            tree_and_node_for_path(&scan.path).ok_or_else(|| CfsError::InvalidPath {
                path: scan.path.clone(),
                reason: "not a /markdown/<name>/{documents,links} table",
            })?;
        self.scan_tree(&name, node)
            .map_err(|_| CfsError::InvalidPath {
                path: scan.path.clone(),
                reason: "no markdown root declared for this tree name",
            })
    }
}

/// Register the `/markdown` surface into BOTH registries when a root is declared.
///
/// Registering both is **load-bearing** (mission acceptance item 5): the `/claude` driver
/// shipped with only its read facet registered and no `engine.mounts.register(...)`, so
/// `DESCRIBE` and the generated docs were true while every SELECT raised `unknown_source` —
/// the pushdown planner resolves against the MOUNT registry (see the claude mission's
/// findings). The engine-level SELECT test below fails if either half un-registers.
pub fn register_markdown_mounts(
    engine: &mut qfs_core::Engine,
    reads: qfs_exec::ReadRegistry,
) -> qfs_exec::ReadRegistry {
    if !has_connections() {
        return reads;
    }
    let _ = engine.mounts.register(Arc::new(markdown_driver()));
    reads.with(
        qfs_core::DriverId::new("markdown"),
        Arc::new(MarkdownReadDriver::open_default()),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_types::Value;
    use tempfile::TempDir;

    /// A fixture tree: nested headings + links, a pre-heading link, frontmatter, a non-md
    /// file, a dot-directory, and a nested subdirectory. Hermetic: a tempdir, no bindings.
    fn fixture_tree() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("plan.md"),
            "---\ntitle: The Plan\nstatus: active\n---\n\n[early](notes/first.md)\n\n# 全体の振り返り\n\n## 懸念\n\nsee [the note](notes/first.md) and [external](https://example.com/x)\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(
            root.join("notes/first.md"),
            "# First note\n\nback to [plan](../plan.md)\n",
        )
        .unwrap();
        // Ignored: not .md, and a dot-directory.
        std::fs::write(root.join("data.csv"), "a,b\n1,2\n").unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join(".hidden/skipped.md"), "# nope\n").unwrap();
        dir
    }

    fn facet_over(dir: &TempDir) -> MarkdownReadDriver {
        MarkdownReadDriver::with_roots([("docs".to_string(), dir.path().to_path_buf())])
    }

    fn col_idx(batch: &qfs_types::Schema, name: &str) -> usize {
        batch
            .columns
            .iter()
            .position(|c| c.name.as_str() == name)
            .expect("column present")
    }

    fn texts(rows: &qfs_exec::RowSet, col: &str) -> Vec<String> {
        let idx = col_idx(&rows.schema, col);
        rows.rows
            .iter()
            .filter_map(|r| match &r.values[idx] {
                Value::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The mission's row-equivalence gate (acceptance item 5, ticket 20260722090300).** The
    /// markdown interpretation rehomed into the codec layer (`qfs_core::decode_markdown_relation`)
    /// reproduces the compiled `qfs-driver-markdown` driver's `documents` and `links` rows —
    /// including `title` derivation, front matter, `target_doc` normalization, and the full nested
    /// `section_path` — byte-for-byte on the shared fixture tree. This is the twin that must be
    /// green before the compiled driver may retire.
    #[test]
    fn codec_relations_are_row_equivalent_to_the_compiled_driver() {
        let dir = fixture_tree();
        // The same set of documents the driver's tree walk scans, root-relative.
        let files = [
            (
                "plan.md",
                std::fs::read(dir.path().join("plan.md")).unwrap(),
            ),
            (
                "notes/first.md",
                std::fs::read(dir.path().join("notes/first.md")).unwrap(),
            ),
        ];
        // Schemas are identical (the codec owns the canonical relation schemas).
        assert_eq!(
            qfs_core::markdown_documents_schema(),
            qfs_driver_markdown::documents_schema()
        );
        assert_eq!(
            qfs_core::markdown_links_schema(),
            qfs_driver_markdown::links_schema()
        );
        for (rel_path, bytes) in &files {
            let text = std::str::from_utf8(bytes).unwrap();
            let driver_doc = qfs_driver_markdown::parse_document(rel_path, text);

            let codec_docs = qfs_core::decode_markdown_relation(
                qfs_core::MarkdownRelation::Documents,
                bytes,
                rel_path,
            )
            .unwrap();
            assert_eq!(
                codec_docs.rows,
                vec![driver_doc.document_row()],
                "documents row for {rel_path} matches the compiled driver"
            );

            let codec_links = qfs_core::decode_markdown_relation(
                qfs_core::MarkdownRelation::Links,
                bytes,
                rel_path,
            )
            .unwrap();
            assert_eq!(
                codec_links.rows,
                driver_doc.link_rows(),
                "links rows for {rel_path} match the compiled driver (section_path + target_doc)"
            );
        }
    }

    /// Build `(engine, reads)` with the /markdown mount + read facet registered — the same
    /// two-registry shape `register_markdown_mounts` wires from bindings.
    fn engine_and_reads(dir: &TempDir) -> (qfs_core::Engine, qfs_exec::ReadRegistry) {
        let mut engine = qfs_core::Engine::new();
        engine
            .mounts
            .register(Arc::new(markdown_driver()))
            .expect("mount /markdown");
        let reads = qfs_exec::ReadRegistry::new().with(
            qfs_core::DriverId::new("markdown"),
            Arc::new(facet_over(dir)),
        );
        (engine, reads)
    }

    fn select(
        engine: &qfs_core::Engine,
        reads: &qfs_exec::ReadRegistry,
        q: &str,
    ) -> qfs_exec::RowSet {
        let stmt = qfs_exec::parse(q).expect("parse");
        qfs_exec::block_on_read(&stmt, &engine.mounts, reads).expect("read through the engine")
    }

    /// **The mission's reachability guard (acceptance item 5)**: `documents` returns real rows
    /// THROUGH the engine — parse → resolve → plan (mount registry) → scan (read registry) —
    /// never the scanner struct directly. This is the test the `/claude` driver lacked: it
    /// fails with `unknown_source` if the `engine.mounts.register` half ever un-registers.
    #[test]
    fn documents_select_through_the_engine_returns_rows() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let rows = select(&engine, &reads, "/markdown/docs/documents |> LIMIT 10");
        assert_eq!(
            texts(&rows, "path"),
            vec!["notes/first.md", "plan.md"],
            "one row per .md file, root-relative, deterministic order; csv + dot-dir ignored"
        );
        assert_eq!(texts(&rows, "title"), vec!["First note", "The Plan"]);
        // The frontmatter summary column carries the parsed map (Json), NULL when absent.
        let fm = col_idx(&rows.schema, "frontmatter");
        assert!(matches!(&rows.rows[0].values[fm], Value::Null));
        match &rows.rows[1].values[fm] {
            Value::Json(v) => {
                assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("active"));
            }
            other => panic!("plan.md frontmatter should be Json, got {other:?}"),
        }
    }

    /// `links` resolves through the engine with the section context preserved (items 3 + 5):
    /// the nested heading path arrives IN ORDER as an array, the pre-heading link carries the
    /// empty path, and `target_doc` is joinable against `documents.path`.
    #[test]
    fn links_select_through_the_engine_preserves_section_paths() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let rows = select(&engine, &reads, "/markdown/docs/links |> LIMIT 100");

        let src = col_idx(&rows.schema, "source_doc");
        let sec = col_idx(&rows.schema, "source_section_path");
        let tgt = col_idx(&rows.schema, "target");
        let tdoc = col_idx(&rows.schema, "target_doc");

        let section = |row: &qfs_types::Row| -> Vec<String> {
            match &row.values[sec] {
                Value::Array(items) => items
                    .iter()
                    .map(|v| match v {
                        Value::Text(s) => s.clone(),
                        other => panic!("section segment should be Text, got {other:?}"),
                    })
                    .collect(),
                other => panic!("source_section_path should be Array, got {other:?}"),
            }
        };

        // notes/first.md sorts first: its link sits under the top-level heading.
        let first: Vec<&qfs_types::Row> = rows
            .rows
            .iter()
            .filter(|r| matches!(&r.values[src], Value::Text(s) if s == "notes/first.md"))
            .collect();
        assert_eq!(first.len(), 1);
        assert_eq!(section(first[0]), vec!["First note"]);
        assert!(matches!(&first[0].values[tdoc], Value::Text(s) if s == "plan.md"));

        let plan: Vec<&qfs_types::Row> = rows
            .rows
            .iter()
            .filter(|r| matches!(&r.values[src], Value::Text(s) if s == "plan.md"))
            .collect();
        assert_eq!(plan.len(), 3);
        // The pre-heading link carries the EMPTY path (never NULL, never guessed).
        assert_eq!(section(plan[0]), Vec::<String>::new());
        // The link under 「懸念」 inside 「全体の振り返り」 carries BOTH levels, in order.
        assert_eq!(section(plan[1]), vec!["全体の振り返り", "懸念"]);
        assert!(matches!(&plan[1].values[tdoc], Value::Text(s) if s == "notes/first.md"));
        // The external link keeps its target as written and is not joinable.
        assert!(matches!(&plan[2].values[tgt], Value::Text(s) if s == "https://example.com/x"));
        assert!(matches!(&plan[2].values[tdoc], Value::Null));

        // Joinability (item 3): every in-tree target_doc equals some documents.path.
        let docs = select(&engine, &reads, "/markdown/docs/documents |> LIMIT 100");
        let doc_paths = texts(&docs, "path");
        for row in &rows.rows {
            if let Value::Text(td) = &row.values[tdoc] {
                assert!(
                    doc_paths.contains(td),
                    "target_doc `{td}` must join documents.path"
                );
            }
        }
    }

    /// A WHERE over the engine re-filters locally (PushdownProfile::None is honest).
    #[test]
    fn where_filters_through_the_engine() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let rows = select(
            &engine,
            &reads,
            "/markdown/docs/documents |> WHERE path == 'plan.md' |> LIMIT 10",
        );
        assert_eq!(texts(&rows, "path"), vec!["plan.md"]);
    }

    /// **The rescan ruling (acceptance item 6)**: the facet is read-through, so after adding,
    /// editing, and removing files the NEXT engine query reflects the change — no stale index
    /// can exist. (If a cache ever lands, this test forces the explicit rescan entry point.)
    #[test]
    fn rescan_via_read_through_reflects_tree_changes() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        assert_eq!(
            select(&engine, &reads, "/markdown/docs/documents |> LIMIT 10")
                .rows
                .len(),
            2
        );

        // Add a file; edit another (new link under a new heading); remove a third.
        std::fs::write(dir.path().join("added.md"), "# Added\n\n[l](plan.md)\n").unwrap();
        std::fs::write(
            dir.path().join("notes/first.md"),
            "# First note\n\n## Edited\n\n[edited](../added.md)\n",
        )
        .unwrap();
        std::fs::remove_file(dir.path().join("plan.md")).unwrap();

        let docs = select(&engine, &reads, "/markdown/docs/documents |> LIMIT 10");
        assert_eq!(texts(&docs, "path"), vec!["added.md", "notes/first.md"]);

        let links = select(&engine, &reads, "/markdown/docs/links |> LIMIT 100");
        let sec = col_idx(&links.schema, "source_section_path");
        let edited: Vec<Vec<String>> = links
            .rows
            .iter()
            .filter_map(|r| match &r.values[sec] {
                Value::Array(items) => Some(
                    items
                        .iter()
                        .filter_map(|v| match v {
                            Value::Text(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect();
        assert!(
            edited.contains(&vec!["First note".to_string(), "Edited".to_string()]),
            "the edited section path is visible on the next query: {edited:?}"
        );
    }

    /// An undeclared tree name fails closed with a structured error (never a panic, never
    /// silent empty rows pretending the tree exists).
    #[test]
    fn undeclared_tree_fails_closed() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let stmt = qfs_exec::parse("/markdown/ghost/documents |> LIMIT 1").expect("parse");
        assert!(qfs_exec::block_on_read(&stmt, &engine.mounts, &reads).is_err());
    }

    /// The declared-drivers convention end-to-end (acceptance items 1 + 5): a persisted
    /// `CONNECT /markdown/docs TO markdown AT '<root>'` binding — the canonical `path_binding`
    /// source, NO env var — wires `has_connections`, BOTH registries via
    /// `register_markdown_mounts`, and a real engine-level SELECT.
    #[test]
    fn path_binding_declaration_wires_the_engine() {
        let _home = crate::testenv::HomeGuard::with_passphrase("markdown-binding-test");
        let dir = fixture_tree();
        assert!(!has_connections(), "fresh home: nothing declared");

        let conn = crate::store::open_system_db()
            .unwrap()
            .unwrap()
            .into_db()
            .into_connection();
        crate::path_binding::db_upsert_binding(
            &conn,
            "/markdown/docs",
            "markdown",
            dir.path().to_str(),
            None,
            Some("local"),
            None,
            None,
        )
        .unwrap();
        drop(conn);

        assert!(
            has_connections(),
            "the declared binding wires the markdown driver"
        );
        let mut engine = qfs_core::Engine::new();
        let reads = register_markdown_mounts(&mut engine, qfs_exec::ReadRegistry::new());
        let rows = select(&engine, &reads, "/markdown/docs/documents |> LIMIT 10");
        assert_eq!(
            rows.rows.len(),
            2,
            "the declared root scans through the engine"
        );
    }
}
