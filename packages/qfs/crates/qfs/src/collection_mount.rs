//! The `/collections/<view>` **read-by-path mount** for registered collection views (mission
//! `a-file-collection-is-a-declared-set-over-any-blob-source`, ticket 20260723100000).
//!
//! ## What this wires
//! A collection is a **declared, named set** registered through the ordinary definition layer:
//! `CREATE VIEW <name> AS /local/<root>/**/*.md |> decode md.<relation>` desugars to a
//! `/server/views` INSERT (blueprint §3, no new grammar). Ticket 20260722090300 shipped the
//! registration read (`qfs_exec::read_registered_collection`) and proved the declared
//! `documents`/`links` views **row-equivalent** to the compiled `/markdown` driver — but only at the
//! helper level. THIS module resolves a registered collection view **by path**: a live
//! `/collections/<view>` query (and `DESCRIBE`) reaches the declared view the way the compiled
//! `/markdown/<name>` mount does today.
//!
//! ## The two facets, the same split the retired compiled `/markdown` driver used
//! [`CollectionsDriver`] is the **pure** introspective half: `describe`/`capabilities` resolve a
//! `/collections/<view>` node to the registered view's markdown-relation schema, owning NO root and
//! NO creds (the `/sys` / `/claude` / `/markdown` `NoopApplier` pattern — READ-ONLY). The impure
//! `/local` scan lives in [`CollectionReadDriver`], the binary's read facet: it resolves the view by
//! name, scans the stored body's `/local` source (materialized), and runs the registration read —
//! `qfs_exec::read_registered_collection`, which applies the root-relative strip (design brief
//! Ruling 3) BEFORE the decode so every row's join id is root-relative, exactly as the compiled
//! driver emits. The generic `decode md.<relation>` query path stays VFS-anchored (Ruling 3); the
//! strip is a property of this registered read-by-path facet alone.
//!
//! ## Where the registered views come from
//! The registered collection views are the `/server/views` rows whose stored body is a `/local` +
//! `decode md.<relation>` collection pipeline ([`collection_views_from_state`]). The serve path
//! ([`crate::serve`]) mounts this surface over the live [`ServerState`], resolving each view lazily
//! at request time — so a view registered over the definition layer becomes reachable by path with
//! no restart. Fail-closed: an unregistered `/collections/<view>` path is a structured error.

use std::sync::{Arc, RwLock};

use qfs_core::{
    markdown_relation_schema, Archetype, Capabilities, CfsError, Driver, DriverId, Engine,
    MarkdownRelation, NodeDesc, Path, PlanApplier, ProcSig, PushdownProfile, RequestContext,
    RowBatch, Verb,
};
use qfs_driver_local::{scan_rows_with, Sandbox};
use qfs_exec::{ReadDriver, ReadRegistry, Statement};
use qfs_provision::ServerState;
use qfs_pushdown::ScanNode;

/// The mount the registered collection-view surface answers under.
pub const COLLECTIONS_MOUNT: &str = "/collections";

/// The read facet's [`DriverId`] (the mount without its leading slash — the source id the planner
/// derives for a `/collections/...` scan, matching [`Driver::id`]'s default).
#[must_use]
fn collections_driver_id() -> DriverId {
    DriverId::new("collections")
}

/// One registered collection view, resolved for read-by-path: its name (the `/collections/<name>`
/// segment), the markdown relation its body decodes to (the `DESCRIBE` schema selector), and the
/// stored body pipeline the read facet executes (`/local/... |> decode md.<relation>`).
#[derive(Clone)]
pub struct CollectionView {
    /// The view name — the `/collections/<name>` addressing segment.
    pub name: String,
    /// The markdown relation the body decodes to (`documents`/`links`).
    pub relation: MarkdownRelation,
    /// The stored body: the collect + `DECODE md.<relation>` pipeline the registration read runs.
    pub body: Statement,
}

impl CollectionView {
    /// Build a [`CollectionView`] from a registered view's `name` and its stored body **source
    /// text** (the `/server/views` `query` column). Returns `None` when the body is not a markdown
    /// collection pipeline (not a read query, or no `DECODE md.<relation>` tail) — a non-collection
    /// view is simply not part of this surface.
    #[must_use]
    pub fn from_source(name: &str, body_src: &str) -> Option<Self> {
        let body = qfs_exec::parse(body_src).ok()?;
        let relation = qfs_exec::collection_relation(&body)?;
        // A collection is declared over a blob path source; a body with no renderable source path
        // cannot be scanned by the /local read facet, so it is not a usable collection view.
        qfs_exec::collection_source_path(&body)?;
        Some(Self {
            name: name.to_string(),
            relation,
            body,
        })
    }
}

/// Derive every registered **collection** view from a [`ServerState`]: the `/server/views` rows whose
/// stored body is a `/local` + `decode md.<relation>` collection pipeline. Non-collection views
/// (a REST/SQL-backed logical view, a materialized cache over a different source) are skipped — this
/// surface is exactly the markdown collection sets.
#[must_use]
pub fn collection_views_from_state(state: &ServerState) -> Vec<CollectionView> {
    state
        .views
        .iter()
        .filter_map(|(name, def)| CollectionView::from_source(name, def.query.as_str()))
        .collect()
}

/// The registered-view resolver the mount + read facet share. Two shapes: a `Static` snapshot (the
/// composition/test seam) and a `Live` handle over the serve process's shared [`ServerState`] lock,
/// resolved lazily per request so a view registered after boot is reachable with no restart.
pub enum ViewSource {
    /// A fixed set of resolved views (the test + explicit-composition seam).
    Static(Vec<CollectionView>),
    /// The live serve-side configuration: resolve the named view from `/server/views` on demand.
    Live(Arc<RwLock<ServerState>>),
}

impl ViewSource {
    /// Resolve one registered collection view by name, `None` when no such collection view is
    /// registered (fail-closed — the caller surfaces a structured unregistered-path error).
    #[must_use]
    fn resolve(&self, name: &str) -> Option<CollectionView> {
        match self {
            Self::Static(views) => views.iter().find(|v| v.name == name).cloned(),
            Self::Live(state) => {
                let guard = state.read().ok()?;
                let def = guard.views.get(name)?;
                CollectionView::from_source(name, def.query.as_str())
            }
        }
    }
}

/// The name segment of a `/collections/<view>` path, `None` for any other shape (the bare mount, a
/// deeper path). A collection view is addressed by exactly `/collections/<view>`.
#[must_use]
fn collection_name_of(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/collections/")?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

/// The PURE introspective driver for `/collections/<view>`: `describe` resolves the registered view's
/// markdown-relation schema (identical to the compiled `/markdown` driver's `DESCRIBE`), and every
/// write verb is rejected at the parse-time capability gate (READ-ONLY). Owns NO root and NO creds —
/// the `/local` scan lives in [`CollectionReadDriver`].
pub struct CollectionsDriver {
    source: Arc<ViewSource>,
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
}

impl CollectionsDriver {
    /// Build the pure describe/plan driver over a shared view source.
    #[must_use]
    pub fn new(source: Arc<ViewSource>) -> Self {
        Self {
            source,
            pushdown: PushdownProfile::None,
            procs: Vec::new(),
        }
    }
}

impl Driver for CollectionsDriver {
    fn mount(&self) -> &str {
        COLLECTIONS_MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, CfsError> {
        let name = collection_name_of(path.as_str()).ok_or_else(|| CfsError::UnsupportedVerb {
            path: path.as_str().to_string(),
            verb: "DESCRIBE",
            supported: Vec::new(),
        })?;
        let view = self
            .source
            .resolve(&name)
            .ok_or_else(|| CfsError::UnsupportedVerb {
                path: path.as_str().to_string(),
                verb: "DESCRIBE",
                supported: Vec::new(),
            })?;
        let desc = NodeDesc::new(
            Archetype::RelationalTable,
            markdown_relation_schema(view.relation),
        );
        // 番地の鍵の宣言 (matching the compiled /markdown driver): a documents row is selected by its
        // `path` value; a links row is an EDGE and declares no child.
        let desc = match view.relation {
            MarkdownRelation::Documents => desc.child_key(["path"]),
            MarkdownRelation::Links => desc,
        };
        Ok(desc)
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        match collection_name_of(path.as_str()).and_then(|n| self.source.resolve(&n)) {
            Some(_) => Capabilities::from_verbs(&[Verb::Select]),
            None => Capabilities::none(),
        }
    }

    fn procedures(&self) -> &[ProcSig] {
        &self.procs
    }

    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }

    fn applier(&self) -> &dyn PlanApplier {
        // READ-ONLY: no write verb passes the capability gate (the /sys / /claude / /markdown
        // NoopApplier pattern), so nothing real ever routes here.
        &NoopApplier
    }
}

/// A no-op applier for the [`Driver::applier`] contract slot (mirrors the `/markdown` driver's): the
/// surface is read-only, so no effect ever reaches it past the parse-time capability gate.
struct NoopApplier;

impl PlanApplier for NoopApplier {
    fn apply(
        &mut self,
        node: &qfs_core::EffectNode,
    ) -> Result<qfs_core::AppliedEffect, qfs_core::ApplyError> {
        Ok(qfs_core::AppliedEffect::new(node.id, 0))
    }
}

/// The async read facet for `/collections/<view>`: resolve the view by name, scan its stored body's
/// `/local` source (materialized — each file's bytes read into `content`), then run the registration
/// read ([`qfs_exec::read_registered_collection`]) which strips the collection root to the
/// root-relative join id BEFORE decoding. The delivered rows are row-equivalent to the compiled
/// `/markdown` driver's over the same files. The `/local` scan is confined to [`Self::sandbox`]
/// (its root is the collection's `/local` mount root).
pub struct CollectionReadDriver {
    source: Arc<ViewSource>,
    sandbox: Sandbox,
}

impl CollectionReadDriver {
    /// Build the read facet over a shared view source and the `/local` sandbox its bodies scan.
    #[must_use]
    pub fn new(source: Arc<ViewSource>, sandbox: Sandbox) -> Self {
        Self { source, sandbox }
    }
}

#[async_trait::async_trait]
impl ReadDriver for CollectionReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let invalid = |reason: &'static str| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason,
        };
        let name = collection_name_of(&scan.path)
            .ok_or_else(|| invalid("not a /collections/<view> path"))?;
        let view = self
            .source
            .resolve(&name)
            .ok_or_else(|| invalid("no registered collection view for this name"))?;
        let source_path = qfs_exec::collection_source_path(&view.body)
            .ok_or_else(|| invalid("registered collection body has no /local source path"))?;
        // The stored body's collect segment, materialized (each file's bytes into `content`) — the
        // same listing the /markdown driver's tree walk sees, sourced through the /local sandbox.
        let scanned = scan_rows_with(&self.sandbox, &source_path, None, true)
            .map_err(|_| invalid("collection /local scan failed"))?;
        // The registration read: strip the collection root (Ruling 3) then decode the relation.
        qfs_exec::read_registered_collection(scanned, &view.body)
            .map_err(|_| invalid("collection registration read failed"))
    }
}

/// Register the `/collections` read-by-path surface into BOTH registries over a shared view source
/// and the `/local` sandbox its bodies scan (the compiled `/markdown` mount's two-registry twin).
/// Registering both is load-bearing: the pushdown planner resolves against the MOUNT registry, and
/// the read executor dispatches the scan through the READ registry — the same two-registry shape the
/// `/markdown` mount uses. Returns the augmented read registry.
#[must_use]
pub fn register_collection_mounts(
    engine: &mut Engine,
    reads: ReadRegistry,
    source: Arc<ViewSource>,
    sandbox: Sandbox,
) -> ReadRegistry {
    let _ = engine
        .mounts
        .register(Arc::new(CollectionsDriver::new(Arc::clone(&source))));
    reads.with(
        collections_driver_id(),
        Arc::new(CollectionReadDriver::new(source, sandbox)),
    )
}

/// The `/local` sandbox root the serve-side collection read facet scans — the daemon's working tree
/// (its current directory), falling back to `/` when the cwd is unavailable. A collection body's
/// `/local/<root>/**/*.md` source resolves under this root, exactly as the interactive shell roots
/// `/local` at the process cwd.
#[must_use]
pub fn serve_local_root() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_core::Schema;
    use qfs_types::Value;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// The shared fixture tree: nested headings + links, a pre-heading link, frontmatter, a non-md
    /// file, a dot-directory. Hermetic: a tempdir, no bindings. The expected documents/links values
    /// are pinned as literals below — the surface's row shape is proven self-contained, with no
    /// dependency on the (retired) compiled `/markdown` driver.
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
        std::fs::write(root.join("data.csv"), "a,b\n1,2\n").unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join(".hidden/skipped.md"), "# nope\n").unwrap();
        dir
    }

    /// The two registered collection views over the fixture's `/local` mount: `docs_documents` and
    /// `docs_links`, each a `/local/**/*.md |> decode md.<relation>` body. Their `/local` root is the
    /// fixture dir, so the glob sees exactly the two `.md` files under it.
    fn fixture_views() -> Vec<CollectionView> {
        vec![
            CollectionView::from_source("docs_documents", "/local/**/*.md |> decode md.documents")
                .unwrap(),
            CollectionView::from_source("docs_links", "/local/**/*.md |> decode md.links").unwrap(),
        ]
    }

    /// Build `(engine, reads)` with the `/collections/<view>` read-by-path mount registered over the
    /// fixture tree — the surface under test, self-contained (no compiled-driver oracle).
    fn engine_and_reads(dir: &TempDir) -> (Engine, ReadRegistry) {
        let mut engine = Engine::new();
        let source = Arc::new(ViewSource::Static(fixture_views()));
        let reads = register_collection_mounts(
            &mut engine,
            ReadRegistry::new(),
            source,
            Sandbox::new(dir.path().to_path_buf()),
        );
        (engine, reads)
    }

    fn select(engine: &Engine, reads: &ReadRegistry, q: &str) -> qfs_exec::RowSet {
        let stmt = qfs_exec::parse(q).expect("parse");
        qfs_exec::block_on_read(&stmt, &engine.mounts, reads).expect("read through the engine")
    }

    fn docs_col(schema: &Schema, name: &str) -> usize {
        schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == name)
            .unwrap_or_else(|| panic!("column {name} present"))
    }

    fn texts(rows: &qfs_exec::RowSet, col: &str) -> Vec<String> {
        let idx = docs_col(&rows.schema, col);
        rows.rows
            .iter()
            .filter_map(|r| match &r.values[idx] {
                Value::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The ticket's live by-path row-equivalence gate (documents).** A live
    /// `/collections/docs_documents` query — parse → resolve (the `/collections` mount) → plan → scan
    /// (the read facet runs `read_registered_collection` over the `/local` body) — delivers, THROUGH
    /// the engine, the exact documents rows the retired compiled `/markdown` driver did: one row per
    /// `.md` file with the root-relative `path` join id, the derived `title`, and the parsed
    /// `frontmatter` (NULL when absent). The values are pinned as literals (self-contained oracle).
    #[test]
    fn live_by_path_documents_pins_path_title_and_frontmatter() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let docs = select(&engine, &reads, "/collections/docs_documents |> LIMIT 100");

        // One row per .md file, root-relative `path`, deterministic order (csv + dot-dir ignored).
        assert_eq!(texts(&docs, "path"), vec!["notes/first.md", "plan.md"]);
        // `title`: notes/first.md from its first ATX heading; plan.md from its frontmatter `title`.
        assert_eq!(texts(&docs, "title"), vec!["First note", "The Plan"]);
        // Front matter: notes/first.md has none (NULL); plan.md carries the parsed map.
        let fm = docs_col(&docs.schema, "frontmatter");
        assert!(matches!(&docs.rows[0].values[fm], Value::Null));
        match &docs.rows[1].values[fm] {
            Value::Json(v) => assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("active")),
            other => panic!("plan.md frontmatter should be Json, got {other:?}"),
        }
    }

    /// **The ticket's live by-path `links` gate (+ self-join).** A live `/collections/docs_links`
    /// query delivers, THROUGH the engine, the same links the retired compiled `/markdown` driver did:
    /// `source_doc`, the full nested `source_section_path` (array, in order; `[]` pre-heading),
    /// `target`/`target_doc` (root-relative normalization; NULL for external), and the 1-based `line`.
    /// The registration additionally prepends the `path` provenance join id (== `source_doc`, Ruling
    /// 3). Every in-tree `target_doc` self-joins against a `documents.path`. Values pinned as literals.
    #[test]
    fn live_by_path_links_pins_sections_targets_and_self_joins_documents_path() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let links = select(&engine, &reads, "/collections/docs_links |> LIMIT 100");

        // The registration prepends `path`, and it equals `source_doc` on every row (Ruling 3).
        assert_eq!(links.schema.columns[0].name.as_str(), "path");
        let src = docs_col(&links.schema, "source_doc");
        let sec = docs_col(&links.schema, "source_section_path");
        let tgt = docs_col(&links.schema, "target");
        let tdoc = docs_col(&links.schema, "target_doc");
        for row in &links.rows {
            assert_eq!(
                row.values[0], row.values[src],
                "the registered links `path` join id equals `source_doc`"
            );
        }
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

        // notes/first.md sorts first: its single link sits under the top-level heading.
        let first: Vec<&qfs_types::Row> = links
            .rows
            .iter()
            .filter(|r| matches!(&r.values[src], Value::Text(s) if s == "notes/first.md"))
            .collect();
        assert_eq!(first.len(), 1);
        assert_eq!(section(first[0]), vec!["First note"]);
        assert!(matches!(&first[0].values[tdoc], Value::Text(s) if s == "plan.md"));

        // plan.md carries three links, in document order.
        let plan: Vec<&qfs_types::Row> = links
            .rows
            .iter()
            .filter(|r| matches!(&r.values[src], Value::Text(s) if s == "plan.md"))
            .collect();
        assert_eq!(plan.len(), 3);
        // The pre-heading link carries the EMPTY section path (never NULL, never guessed).
        assert_eq!(section(plan[0]), Vec::<String>::new());
        // The link under 「懸念」 inside 「全体の振り返り」 carries BOTH levels, in order.
        assert_eq!(section(plan[1]), vec!["全体の振り返り", "懸念"]);
        assert!(matches!(&plan[1].values[tdoc], Value::Text(s) if s == "notes/first.md"));
        // The external link keeps its target as written and is not joinable (NULL target_doc).
        assert!(matches!(&plan[2].values[tgt], Value::Text(s) if s == "https://example.com/x"));
        assert!(matches!(&plan[2].values[tdoc], Value::Null));

        // Self-join (item 3): every in-tree target_doc equals some documents.path — through the
        // /collections surface alone (both sides read by path).
        let docs = select(&engine, &reads, "/collections/docs_documents |> LIMIT 100");
        let doc_paths = texts(&docs, "path");
        for row in &links.rows {
            if let Value::Text(td) = &row.values[tdoc] {
                assert!(
                    doc_paths.contains(td),
                    "links.target_doc `{td}` must self-join /collections documents.path"
                );
            }
        }
    }

    /// **The ticket's `DESCRIBE`-by-path gate.** `DESCRIBE /collections/<view>` reports the canonical
    /// markdown-relation schema — the SAME `qfs_exec::markdown_relation_describe_schema` the retired
    /// compiled `/markdown` driver's `DESCRIBE` reported — so an agent/viewer discovers the declared
    /// set's shape generically. `documents` additionally declares the `@path` child address.
    #[test]
    fn describe_by_path_reports_the_relation_schemas() {
        let source = Arc::new(ViewSource::Static(fixture_views()));
        let driver = CollectionsDriver::new(source);

        // Reference descs built via the same public NodeDesc API (avoids naming the ChildAddress type,
        // which the binary does not import): documents declares the `@path` child key; links declares
        // no child (the default).
        let docs_schema = qfs_exec::markdown_relation_describe_schema(MarkdownRelation::Documents);
        let links_schema = qfs_exec::markdown_relation_describe_schema(MarkdownRelation::Links);
        let want_docs =
            NodeDesc::new(Archetype::RelationalTable, docs_schema.clone()).child_key(["path"]);
        let want_links = NodeDesc::new(Archetype::RelationalTable, links_schema.clone());

        let coll_docs = driver
            .describe(&Path::new("/collections/docs_documents"))
            .unwrap();
        assert_eq!(
            coll_docs.schema, docs_schema,
            "DESCRIBE /collections/docs_documents == the canonical documents schema"
        );
        assert_eq!(
            coll_docs.child_address, want_docs.child_address,
            "documents declares the `@path` child address"
        );

        let coll_links = driver
            .describe(&Path::new("/collections/docs_links"))
            .unwrap();
        assert_eq!(
            coll_links.schema, links_schema,
            "DESCRIBE /collections/docs_links == the canonical links schema"
        );
        assert_eq!(
            coll_links.child_address, want_links.child_address,
            "a links row is an edge and declares no child"
        );
    }

    /// An unregistered `/collections/<view>` fails closed — a structured error, never silent empty
    /// rows pretending the view exists.
    #[test]
    fn unregistered_view_fails_closed() {
        let dir = fixture_tree();
        let (engine, reads) = engine_and_reads(&dir);
        let stmt = qfs_exec::parse("/collections/ghost |> LIMIT 1").expect("parse");
        assert!(qfs_exec::block_on_read(&stmt, &engine.mounts, &reads).is_err());
    }

    /// The `/server/views` bridge (the definition layer → this surface): a [`ServerState`] carrying a
    /// CREATE-VIEW-desugared collection view yields exactly the collection views, skipping a
    /// non-collection (REST-backed) view — so registration through the ordinary definition layer
    /// makes the view reachable by path, with no new grammar.
    #[test]
    fn collection_views_derive_from_server_state_skipping_non_collections() {
        use qfs_provision::{StatementSource, ViewDef};
        let mut state = ServerState::new();
        let view = |name: &str, q: &str| ViewDef {
            name: name.to_string(),
            query: StatementSource::new(q),
            materialized: false,
            last_run: None,
            cache_json: None,
        };
        state.views.insert(
            "docs_documents".to_string(),
            view(
                "docs_documents",
                "/local/docs/**/*.md |> decode md.documents",
            ),
        );
        state.views.insert(
            "docs_links".to_string(),
            view("docs_links", "/local/docs/**/*.md |> decode md.links"),
        );
        // A non-collection logical view (no md.<relation> decode) is NOT part of this surface.
        state.views.insert(
            "recent_mail".to_string(),
            view("recent_mail", "/mail/inbox |> LIMIT 10"),
        );

        let mut derived: Vec<(String, MarkdownRelation)> = collection_views_from_state(&state)
            .into_iter()
            .map(|v| (v.name, v.relation))
            .collect();
        derived.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            derived,
            vec![
                ("docs_documents".to_string(), MarkdownRelation::Documents),
                ("docs_links".to_string(), MarkdownRelation::Links),
            ],
            "only the /local + decode md.<relation> collection views are derived"
        );
    }

    /// The `Live` view source resolves a view registered in a shared [`ServerState`] AFTER the mount
    /// was built (the serve topology: the mount is wired before boot replay populates the state), so
    /// a view registered over the definition layer is reachable by path with no restart.
    #[test]
    fn live_source_resolves_a_view_registered_after_wiring() {
        use qfs_provision::{StatementSource, ViewDef};
        let dir = fixture_tree();
        let state = Arc::new(RwLock::new(ServerState::new()));
        let source = Arc::new(ViewSource::Live(Arc::clone(&state)));
        let mut engine = Engine::new();
        let reads = register_collection_mounts(
            &mut engine,
            ReadRegistry::new(),
            source,
            Sandbox::new(dir.path().to_path_buf()),
        );

        // Register the view AFTER wiring — the Live source resolves it lazily at request time.
        state.write().unwrap().views.insert(
            "docs_documents".to_string(),
            ViewDef {
                name: "docs_documents".to_string(),
                query: StatementSource::new("/local/**/*.md |> decode md.documents"),
                materialized: false,
                last_run: None,
                cache_json: None,
            },
        );

        let coll = select(&engine, &reads, "/collections/docs_documents |> LIMIT 100");
        assert_eq!(
            texts(&coll, "path"),
            vec!["notes/first.md", "plan.md"],
            "the lazily-resolved live view reads the fixture's documents by path"
        );
    }

    /// The serve default sandbox root helper does not panic and yields an absolute path.
    #[test]
    fn serve_sandbox_root_is_absolute() {
        let root: PathBuf = serve_local_root();
        assert!(
            root.is_absolute(),
            "the /local root for the serve facet is absolute: {root:?}"
        );
    }
}
