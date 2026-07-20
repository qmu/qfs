//! `qfs-driver-markdown` — the **markdown collection-path driver** (the strategy plan's
//! マークダウン収集パス, minimal slice; mission
//! `markdown-trees-are-queryable-as-documents-and-links-tables`).
//!
//! A declared tree of `.md` files is an ordinary queryable qfs surface: one `CONNECT
//! /markdown/<name> TO markdown AT '<root-dir>'` binding (the declared-drivers convention — a
//! committed, reviewable `path_binding` row; **no `QFS_*` env var**) resolves exactly two
//! relational tables:
//!
//! - **`/markdown/<name>/documents`** — one row per `.md` file: root-relative `path` (the
//!   canonical join id), `title` (frontmatter `title`, else first ATX heading, else NULL), and
//!   the whole parsed YAML `frontmatter` as one Json column.
//! - **`/markdown/<name>/links`** — one row per inline markdown link: `source_doc`,
//!   `source_section_path` (the **full nested heading path** of the section containing the
//!   link, top-level first, as a lossless `Array(Text)`; `[]` for a pre-heading link),
//!   `target` (as written), `target_doc` (the normalized root-relative form joinable against
//!   `documents.path`; NULL for external/escaping targets), and the 1-based `line`.
//!
//! ## No typing, by construction (mission acceptance item 4)
//! The `links` schema carries **no relation-type column** and nothing here infers semantics
//! from heading text. The closed relation vocabulary (`parent`/`concerns`/`references`/… —
//! "declare and reject, never guess"（推測するな、宣言して拒否せよ）) is a **later, separate
//! mission** layered on the preserved `source_section_path`; this slice's whole job is to
//! record that context losslessly so that mission stays possible.
//!
//! ## The same split as the `/sys` / `/claude` / `/directories` drivers
//! [`MarkdownDriver`]'s **introspective** half (describe/capabilities/pushdown) is **pure** —
//! static, credential-free schemas with NO filesystem root bound — and its `applier()` is a
//! [`NoopApplier`] (READ-ONLY: every write verb is rejected at the parse-time capability gate).
//! The impure tree walk (`std::fs`) lives in the qfs binary leaf, which feeds file text into
//! this crate's pure [`parse::parse_document`]; the crate itself stays I/O-free, tokio-free and
//! wasm-buildable, so every parsing behavior is pinned by hermetic string-fixture tests.
//!
//! ## Parser scope (documented, per the design brief)
//! ATX headings only; inline `[text](target)` links only (images, autolinks and
//! reference-style links excluded); fenced code blocks excluded. See [`parse`] for details.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod parse;
mod schema;

use qfs_driver::{Archetype, Capabilities, Driver, NodeDesc, Path, ProcSig, PushdownProfile, Verb};
use qfs_plan::PlanApplier;

pub use parse::{documents_schema, links_schema, parse_document, ParsedDocument, ParsedLink};
pub use schema::{
    markdown_node_schema, node_for_path, tree_and_node_for_path, MarkdownNode, MARKDOWN_MOUNT,
};

/// The markdown collection-path driver. Pure introspection only — it owns NO root and NO
/// filesystem handle (the tree walk is injected from the binary's read facet). Construct with
/// [`MarkdownDriver::new`].
pub struct MarkdownDriver {
    // The tables are re-scanned in-engine on every read (the stateless read-through ruling);
    // nothing is pushed down (honest declaration) — filtering is the engine's work.
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
}

impl Default for MarkdownDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownDriver {
    /// Construct the (pure) markdown collection-path driver.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pushdown: PushdownProfile::None,
            procs: Vec::new(),
        }
    }
}

/// The per-node capability set: both tables are READ-ONLY (`SELECT`); every write verb is
/// rejected structurally at the parse-time gate. Single source of truth shared by
/// [`Driver::capabilities`] and the gate.
#[must_use]
pub fn markdown_node_capabilities(node: Option<MarkdownNode>) -> Capabilities {
    match node {
        Some(_) => Capabilities::from_verbs(&[Verb::Select]),
        None => Capabilities::none(),
    }
}

impl Driver for MarkdownDriver {
    fn mount(&self) -> &str {
        MARKDOWN_MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, qfs_driver::CfsError> {
        // Pure: returns static schema data; never walks a directory or reads a file.
        let node =
            node_for_path(path.as_str()).ok_or_else(|| qfs_driver::CfsError::UnsupportedVerb {
                path: path.as_str().to_string(),
                verb: "DESCRIBE",
                supported: Vec::new(),
            })?;
        let desc = NodeDesc::new(Archetype::RelationalTable, markdown_node_schema(node));
        // 番地の鍵の宣言: a document row is selected by its `path` value
        // (`…/documents/@<path>`, percent-encoded). A links row is an EDGE, not a tree
        // node — it declares no child.
        let desc = match node {
            MarkdownNode::Documents => desc.child_key(["path"]),
            _ => desc,
        };
        Ok(desc)
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        markdown_node_capabilities(node_for_path(path.as_str()))
    }

    fn procedures(&self) -> &[ProcSig] {
        &self.procs
    }

    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }

    fn applier(&self) -> &dyn PlanApplier {
        // READ-ONLY: no write verb passes the capability gate, so nothing real can route here;
        // the slot exists only to satisfy the trait (the /sys / /claude NoopApplier pattern).
        &NoopApplier
    }
}

/// A no-op applier for the `Driver::applier()` contract slot (mirrors `SysDriver`'s): the
/// driver is read-only, so no effect ever reaches it past the parse-time capability gate.
struct NoopApplier;

impl PlanApplier for NoopApplier {
    fn apply(
        &mut self,
        node: &qfs_plan::EffectNode,
    ) -> Result<qfs_plan::AppliedEffect, qfs_plan::ApplyError> {
        Ok(qfs_plan::AppliedEffect::new(node.id, 0))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use qfs_driver::{check_capability, CfsError};

    /// G3 — the purity proof for the introspective half: `DESCRIBE` resolves both tables to
    /// stable typed schemas with NO root bound and NO creds — none of these methods take
    /// `&mut self`, return a future, or perform I/O, so a no-I/O round-trip IS the proof.
    #[test]
    fn describe_both_tables_is_pure_no_root_no_creds() {
        let d = MarkdownDriver::new();
        assert_eq!(d.mount(), "/markdown");

        let docs = d.describe(&Path::new("/markdown/docs/documents")).unwrap();
        assert_eq!(docs.archetype, Archetype::RelationalTable);
        assert!(docs.schema.column("path").is_some());
        assert!(docs.schema.column("title").is_some());
        assert!(docs.schema.column("frontmatter").is_some());

        let links = d.describe(&Path::new("/markdown/docs/links")).unwrap();
        assert_eq!(links.archetype, Archetype::RelationalTable);
        assert!(links.schema.column("source_section_path").is_some());
        assert!(links.schema.column("target_doc").is_some());

        // The mount itself, a bare tree, and unknown segments are not describable (no panic).
        assert!(d.describe(&Path::new("/markdown")).is_err());
        assert!(d.describe(&Path::new("/markdown/docs")).is_err());
        assert!(d.describe(&Path::new("/markdown/docs/nope")).is_err());
    }

    /// 番地の鍵の宣言: a document row is selected by its `path` value
    /// (`/markdown/docs/documents/@<path>`); the links table declares no child (an edge
    /// row is not a tree node).
    #[test]
    fn describe_declares_child_addresses_for_markdown_tables() {
        let d = MarkdownDriver::new();
        assert_eq!(
            d.describe(&Path::new("/markdown/docs/documents"))
                .unwrap()
                .child_address,
            qfs_driver::ChildAddress::Key {
                columns: vec!["path".to_string()]
            }
        );
        assert_eq!(
            d.describe(&Path::new("/markdown/docs/links"))
                .unwrap()
                .child_address,
            qfs_driver::ChildAddress::None
        );
    }

    /// Capability golden gate: both tables are READ-ONLY — every write verb is rejected at the
    /// parse-time gate with a structured error, while `SELECT` passes (item 4's structural
    /// half: there is no writable surface a typed edge could sneak in through).
    #[test]
    fn both_tables_are_read_only() {
        let d = MarkdownDriver::new();
        for table in ["documents", "links"] {
            let path = Path::new(format!("/markdown/docs/{table}"));
            assert!(check_capability(&d, &path, Verb::Select).is_ok());
            for verb in [Verb::Insert, Verb::Upsert, Verb::Update, Verb::Remove] {
                let err = check_capability(&d, &path, verb).unwrap_err();
                assert!(
                    matches!(err, CfsError::UnsupportedVerb { .. }),
                    "/markdown/docs/{table} must reject {} structurally",
                    verb.label()
                );
            }
        }
    }

    /// The driver is object-safe (`Arc<dyn Driver>`) — the registries store trait objects (G2).
    #[test]
    fn markdown_driver_is_object_safe() {
        let d: Arc<dyn Driver> = Arc::new(MarkdownDriver::new());
        assert_eq!(d.mount(), "/markdown");
        let _seam: &dyn PlanApplier = d.applier();
    }
}
