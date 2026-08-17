//! blueprint §13 — the declared driver's **describe surface** (ticket 20260728085253).
//!
//! A declared driver mounts its describe facet on the stock `qfs-driver-http` [`RestDriver`], whose
//! honest answer for a generic `/rest/<api>/<resource>` node is "weakly typed JSON": one
//! `value: Json` column, no children. For a *declared* mount that answer is wrong twice over, and
//! both halves break the documented agent loop:
//!
//! - **The mount root advertised no children.** `DESCRIBE /chatwork` reported
//!   [`ChildAddress::None`] and nothing pointed at `/chatwork/rooms`, so there was no path from
//!   "the driver is mounted" to "here is its surface" short of reading the `.qfs` source.
//! - **A declared node reported one synthetic `value: Json` column** even though its
//!   `CREATE VIEW … OF <type>` names the real columns and `run` on the same path delivers exactly
//!   those. `describe` and `run` disagreed about the same node.
//!
//! The DSL already knows both answers. This module reads them off the declaration instead of
//! inventing one: [`DeclaredSurface`] indexes the driver's view templates, and
//! [`DeclaredDescribeDriver`] answers `describe` from that index — the declared `OF` type's
//! [`Schema`] for a view node, the declared child segments (`{param}` ones legible as such) for
//! every node on the way down.
//!
//! ## What it does NOT change
//! `capabilities`, `procedures`, `pushdown`, `write_irreversible` and `applier` all delegate to the
//! inner [`RestDriver`] untouched: the verb gate is the resource config's business, and this is a
//! describe fix, not a capability one. Purity is preserved end to end — building the index reads
//! only `/sys/drivers` rows already loaded, and answering a describe touches no World.

use std::sync::Arc;

use qfs_core::{
    Archetype, Capabilities, CfsError, ChildNode, Column, DeclaredTypeDefs, Driver, NodeDesc, Path,
    PlanApplier, ProcSig, PushdownProfile, Schema, Verb,
};

use crate::declared_driver::{DeclaredDriver, DeclaredNode};

/// One node of a declared driver's surface: the mount-path template split into segments, plus the
/// row shape the declaration promises there.
#[derive(Debug, Clone)]
struct SurfaceNode {
    /// The template's segments **including** the leading driver segment
    /// (`["chatwork", "rooms", "{room}", "messages"]`), so a prefix walk starts at the mount root.
    segments: Vec<String>,
    /// The declared `OF <type>` name (`chatwork/room`), when the view declared one.
    of_type: Option<String>,
    /// The columns this node delivers: the `OF` type's resolved schema, else the statically
    /// knowable shape of the body (`qfs_exec::declared::body_delivered_columns`), else empty.
    columns: Vec<Column>,
}

/// The index a declared driver's `describe` is answered from: every declared view template, in
/// declaration order.
///
/// Built once per mount and shared, because the describe registry registers one mount per binding
/// and the index is pure data.
#[derive(Debug, Clone, Default)]
pub(crate) struct DeclaredSurface {
    nodes: Vec<SurfaceNode>,
}

impl DeclaredSurface {
    /// Index a declared driver's views, resolving each `OF <type>` against the declared-type
    /// registry so a node reports the SAME column set a read of it delivers.
    ///
    /// A type that does not resolve (no row, or a stale body) contributes no columns but keeps its
    /// `of_type` name: the node then reports "contract `chatwork/room`, no columns", which is the
    /// honest description of exactly the state `eval_view_body` refuses at read time.
    pub(crate) fn index(d: &DeclaredDriver, types: &DeclaredTypeDefs) -> Self {
        Self {
            nodes: d.views.iter().map(|v| node_of(v, types)).collect(),
        }
    }

    /// Describe `rel` — a mount-relative path whose leading segment is the driver name
    /// (`/chatwork/rooms/{room}/messages`, or the same with a concrete room id bound).
    ///
    /// `None` when the path names nothing the declaration knows: neither a view nor a walkable
    /// interior on the way to one. The caller then falls back to the inner driver rather than
    /// asserting a shape.
    fn describe(&self, rel: &str) -> Option<NodeDesc> {
        let asked = segments_of(rel);
        let exact = self.nodes.iter().find(|n| matches_exactly(n, &asked));
        let children = self.children_of(&asked);
        if exact.is_none() && children.is_empty() {
            return None;
        }
        let columns = exact.map(|n| n.columns.clone()).unwrap_or_default();
        // A node with declared children below it is an interior you can walk into, whether or not
        // it also delivers rows of its own (`/chatwork/rooms` is both).
        let navigable = !children.is_empty();
        let mut desc = NodeDesc::new(Archetype::RelationalTable, Schema::new(columns))
            .navigable(navigable)
            .with_children(children);
        if let Some(of_type) = exact.and_then(|n| n.of_type.clone()) {
            desc = desc.row_contract(of_type);
        }
        Some(desc)
    }

    /// The distinct child segments declared directly beneath `asked`, in declaration order. A
    /// segment is a `node` when some template ENDS one step below `asked` — i.e. the child is
    /// itself readable, not merely a segment to walk through.
    fn children_of(&self, asked: &[String]) -> Vec<ChildNode> {
        let depth = asked.len();
        let mut out: Vec<ChildNode> = Vec::new();
        for n in &self.nodes {
            if n.segments.len() <= depth || !matches_prefix(n, asked) {
                continue;
            }
            let seg = &n.segments[depth];
            let is_node = n.segments.len() == depth + 1;
            if let Some(existing) = out.iter_mut().find(|c| c.segment == *seg) {
                // A segment reached by several templates is one child; it is a node if ANY of them
                // stops there (`/slack/{ws}/dms/{user}` is a view AND the parent of `…/messages`).
                existing.node |= is_node;
                continue;
            }
            out.push(match qfs_exec::declared::param_name(seg) {
                Some(param) => ChildNode::param(param, is_node),
                None => ChildNode::literal(seg, is_node),
            });
        }
        out
    }
}

/// Index one declared view.
fn node_of(v: &DeclaredNode, types: &DeclaredTypeDefs) -> SurfaceNode {
    let columns = match v.of_type.as_deref() {
        // The declared contract, resolved to real typed columns — the same names `run` delivers,
        // because `shape_to_type` projects the read to exactly this column list.
        Some(of_type) => types
            .get(of_type)
            .map(|def| def.schema.columns.clone())
            .unwrap_or_default(),
        // No `OF`: report only what the body itself decides, and nothing when it decides nothing.
        None => qfs_exec::declared::body_delivered_columns(&v.body).unwrap_or_default(),
    };
    SurfaceNode {
        segments: segments_of(&v.path),
        of_type: v.of_type.clone(),
        columns,
    }
}

/// Split a path into its segments, dropping the leading/trailing slashes and any empty segment
/// (so the mount root `/chatwork` is `["chatwork"]` and `/` is `[]`).
fn segments_of(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Whether `asked` addresses exactly this node: same length, and every segment either equal or
/// bound by a `{param}` placeholder. A caller may spell the placeholder literally (`{room}`, as it
/// appears in the declaration) or bind it (`123`) — both address the node.
fn matches_exactly(n: &SurfaceNode, asked: &[String]) -> bool {
    n.segments.len() == asked.len() && matches_prefix(n, asked)
}

/// Whether `asked` is a (possibly complete) prefix of this node's template, under the same
/// segment-matching rule as [`matches_exactly`].
fn matches_prefix(n: &SurfaceNode, asked: &[String]) -> bool {
    asked.len() <= n.segments.len()
        && n.segments
            .iter()
            .zip(asked)
            .all(|(t, a)| qfs_exec::declared::param_name(t).is_some() || t == a)
}

/// The describe facet of a declared mount: [`DeclaredSurface`] for `describe`, the inner stock
/// [`RestDriver`](qfs_driver_http::RestDriver) for everything else.
///
/// Registered behind the `/rest/<name>` remap like the driver it wraps, so an inbound path arrives
/// as `/rest/<name>/…`; the mount-relative view path is recovered with the same
/// `view_path_of_scan` the read facet uses, so describe and read agree on which node a path names.
pub(crate) struct DeclaredDescribeDriver {
    surface: DeclaredSurface,
    inner: Arc<dyn Driver>,
}

impl DeclaredDescribeDriver {
    /// Wrap `inner` (the cred-free `RestDriver` built from the declaration) with `surface`.
    pub(crate) fn new(surface: DeclaredSurface, inner: Arc<dyn Driver>) -> Self {
        Self { surface, inner }
    }
}

impl Driver for DeclaredDescribeDriver {
    fn mount(&self) -> &str {
        self.inner.mount()
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, CfsError> {
        let rel = qfs_exec::declared::view_path_of_scan(path.as_str());
        match self.surface.describe(&rel) {
            Some(desc) => Ok(desc),
            // A path the declaration does not know: the inner driver's answer stands, so a mount
            // that also serves generic `/rest` resources keeps describing them as before.
            None => self.inner.describe(path),
        }
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        self.inner.capabilities(path)
    }

    fn procedures(&self) -> &[ProcSig] {
        self.inner.procedures()
    }

    fn pushdown(&self) -> &PushdownProfile {
        self.inner.pushdown()
    }

    fn write_irreversible(&self, path: &Path, verb: Verb) -> bool {
        self.inner.write_irreversible(path, verb)
    }

    fn applier(&self) -> &dyn PlanApplier {
        self.inner.applier()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    use crate::declared_driver::{declared_describe_mount, shipped_declared, ShippedDeclaration};
    use qfs_core::DescribeReport;

    /// Describe one path through the SAME surface `qfs describe` builds: the declared mount wrapped
    /// in its `/rest/<name>` remap, registered in a [`qfs_core::MountRegistry`] and resolved by
    /// path, folded into the agent-facing report. Going through the registry (rather than calling
    /// the mount directly) is deliberate — the mount ROOT resolving at all is half of gate 2.
    fn report(binding: &str, decl: &ShippedDeclaration, path: &str) -> DescribeReport {
        let d = decl
            .drivers
            .first()
            .expect("the shipped script declares a driver");
        let mount = declared_describe_mount(binding, d, &decl.type_defs)
            .expect("the declared describe mount builds");
        let mut reg = qfs_core::MountRegistry::new();
        reg.register(Arc::new(mount)).expect("registers");
        let (driver, _) = reg
            .resolve_path(path)
            .unwrap_or_else(|| panic!("{path} resolves to the declared mount"));
        DescribeReport::from_driver(driver.as_ref(), &Path::new(path.to_string()))
            .unwrap_or_else(|e| panic!("describe {path}: {e:?}"))
    }

    fn column_names(r: &DescribeReport) -> Vec<String> {
        r.columns.iter().map(|c| c.name.to_string()).collect()
    }

    /// Gate 1, over the shipped `chatwork.qfs`: for EVERY declared view, the column set `describe`
    /// reports is the column set a read of the same node delivers.
    ///
    /// The two sides are computed by the two production paths, not by one shared constant: the
    /// describe side folds through [`DeclaredSurface`], and the read side is
    /// `declared_eval::view_specs`' `of_columns` — the exact list `eval_view_body`'s `shape_to_type`
    /// projects every delivered row to. Before this ticket the describe side was a synthetic
    /// `value: Json` for all of them, so `describe` and `run` disagreed about every declared node.
    #[test]
    fn describe_of_a_declared_node_reports_the_columns_a_read_delivers_chatwork() {
        assert_describe_matches_read(qfs_skill::CHATWORK_DRIVER, "/chatwork");
    }

    /// Gate 1's second declaration (`slack_driver.qfs`), so the agreement is a property of the
    /// declared machinery and not of one hand-tuned script.
    #[test]
    fn describe_of_a_declared_node_reports_the_columns_a_read_delivers_slack() {
        assert_describe_matches_read(qfs_skill::SLACK_DRIVER, "/slackd");
    }

    fn assert_describe_matches_read(script: &str, binding: &str) {
        let decl = shipped_declared(script);
        let d = decl.drivers.first().expect("one declared driver");
        let specs = crate::declared_eval::view_specs(d, &decl.types);
        assert!(!specs.is_empty(), "the script declares views");
        let mut checked = 0usize;
        for spec in &specs {
            let Some(read_columns) = spec.of_columns.as_ref() else {
                continue; // a no-`OF` view: covered by the blob test below
            };
            assert!(
                !read_columns.is_empty(),
                "{}: the declared OF type resolves columns (a read would refuse otherwise)",
                spec.template
            );
            // The mount binding may differ from the driver name; the view template is spelled in
            // the driver's own namespace, so re-root it at the binding to address the mount.
            let path = rebind(&spec.template, &d.name, binding);
            let r = report(binding, &decl, &path);
            assert_eq!(
                &column_names(&r),
                read_columns,
                "{path}: describe must report the columns a read of it delivers"
            );
            assert!(
                r.row_contract.is_some(),
                "{path}: the report names the declared OF contract"
            );
            assert!(
                !column_names(&r).contains(&"value".to_string()),
                "{path}: the synthetic `value: Json` column is gone"
            );
            checked += 1;
        }
        assert!(
            checked >= 4,
            "several views were actually checked: {checked}"
        );
    }

    /// Re-root a view template from the driver's own namespace onto the mount binding.
    fn rebind(template: &str, driver: &str, binding: &str) -> String {
        template
            .strip_prefix(&format!("/{driver}"))
            .map_or_else(|| template.to_string(), |rest| format!("{binding}{rest}"))
    }

    /// Gate 2: the mount root enumerates its declared children, and the surface is WALKABLE from
    /// there to a leaf using nothing but `describe` output — the loop `SKILL.md` documents and the
    /// declared drivers could not answer (`child_address: none`, every verb false, nothing pointing
    /// at `/chatwork/rooms`).
    #[test]
    fn a_declared_surface_is_walkable_from_the_mount_root_using_only_describe() {
        let decl = shipped_declared(qfs_skill::CHATWORK_DRIVER);
        let root = report("/chatwork", &decl, "/chatwork");
        assert_eq!(
            root.children
                .iter()
                .map(|c| c.path.clone())
                .collect::<Vec<_>>(),
            vec!["/chatwork/rooms".to_string()],
            "the mount root points at its one declared child"
        );

        // Walk down to the messages log following ONLY the child links each report hands back.
        let target = "/chatwork/rooms/{room}/messages";
        let mut at = root;
        let mut visited = vec![at.path.clone()];
        while at.path != target {
            let next = at
                .children
                .iter()
                .find(|c| target.starts_with(&c.path))
                .unwrap_or_else(|| {
                    panic!(
                        "a child of {} leads to {target}: {:?}",
                        at.path, at.children
                    )
                });
            at = report("/chatwork", &decl, &next.path);
            visited.push(at.path.clone());
            assert!(visited.len() < 10, "the walk terminates: {visited:?}");
        }
        assert_eq!(
            visited,
            vec![
                "/chatwork".to_string(),
                "/chatwork/rooms".to_string(),
                "/chatwork/rooms/{room}".to_string(),
                "/chatwork/rooms/{room}/messages".to_string(),
            ]
        );
        assert!(
            !at.columns.is_empty() && at.row_contract.is_some(),
            "the leaf reached by walking is a real, typed node"
        );

        // A parameterised child is legible AS one — the caller learns a room id goes there.
        let rooms = report("/chatwork", &decl, "/chatwork/rooms");
        let room = rooms
            .children
            .iter()
            .find(|c| c.param.is_some())
            .expect("the room segment is parameterised");
        assert_eq!(room.segment, "{room}");
        assert_eq!(room.param.as_deref(), Some("room"));
        assert!(
            !room.node,
            "`/chatwork/rooms/{{room}}` is an interior to walk through, not a declared view"
        );
        // …and `/chatwork/rooms` is itself BOTH a readable node and an interior: it delivers rows
        // of its own AND is the way down to the per-room surface.
        assert!(!rooms.columns.is_empty(), "it delivers rows of its own");
        assert!(
            rooms.children.len() == 1,
            "and it is walkable: {:?}",
            rooms.children
        );
        // One step further down, the per-room interior's children ARE nodes (the message logs and
        // the file listing), which is what makes `node` worth reporting at all.
        let in_room = report("/chatwork", &decl, "/chatwork/rooms/{room}");
        assert!(
            in_room.children.iter().all(|c| c.node),
            "every child of the room interior is itself readable: {:?}",
            in_room.children
        );
    }

    /// A declared view with no `OF` type says so — an empty column list and no row contract —
    /// instead of the synthetic `value: Json` that used to stand in for "we did not look". The
    /// shipped blob view's shape IS knowable (a bytes `FOLLOW` delivers `content`), so that is what
    /// it reports: the same one-column batch a read of it returns.
    #[test]
    fn a_view_with_no_of_type_reports_its_real_shape_never_a_synthetic_value_column() {
        let decl = shipped_declared(qfs_skill::CHATWORK_DRIVER);
        let blob = report(
            "/chatwork",
            &decl,
            "/chatwork/rooms/{room}/files/{file}/blob",
        );
        assert_eq!(column_names(&blob), vec!["content".to_string()]);
        assert_eq!(
            blob.row_contract, None,
            "the view declares no OF type, and the report says so"
        );
        assert!(
            blob.children.is_empty(),
            "a blob leaf has no declared children"
        );
    }

    /// The index is pure data over the declaration: a path the declaration does not know falls
    /// through to the inner driver rather than being asserted into existence.
    #[test]
    fn an_undeclared_path_under_the_mount_falls_through_to_the_inner_driver() {
        let decl = shipped_declared(qfs_skill::CHATWORK_DRIVER);
        let r = report("/chatwork", &decl, "/chatwork/nothing-declared-here");
        assert!(r.children.is_empty());
        assert_eq!(r.row_contract, None);
        // The inner stock RestDriver's honest generic answer, unchanged for a node the declaration
        // never mentions.
        assert_eq!(column_names(&r), vec!["value".to_string()]);
    }

    /// Segment matching: a caller may address a parameterised node by the placeholder spelling the
    /// declaration uses, or by a bound value — both name the same node.
    #[test]
    fn a_param_segment_matches_both_the_placeholder_and_a_bound_value() {
        let decl = shipped_declared(qfs_skill::CHATWORK_DRIVER);
        let template = report("/chatwork", &decl, "/chatwork/rooms/{room}/messages");
        let bound = report("/chatwork", &decl, "/chatwork/rooms/12345/messages");
        assert_eq!(column_names(&template), column_names(&bound));
        assert_eq!(template.row_contract, bound.row_contract);
        // The bound address keeps its own identity in the report and in its children's paths.
        assert_eq!(bound.path, "/chatwork/rooms/12345/messages");
        assert!(bound
            .children
            .iter()
            .all(|c| c.path.starts_with("/chatwork/rooms/12345/messages/")));
    }
}
