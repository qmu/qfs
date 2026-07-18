//! The **pushdown integration seam** (ticket t14): wire a query AST through the
//! `qfs-pushdown` planner using the live [`MountRegistry`], so independent native
//! [`ScanNode`](qfs_pushdown::ScanNode)s surface for T10's batcher.
//!
//! This is the boundary the t14 ticket asks for: `partition_by_source → engine` wired
//! into the engine. `qfs-core` owns the registry (the source of driver pushdown profiles
//! and `describe` schemas), so it is the right home for the adapter that turns the
//! registry into a `qfs_pushdown::SourceRegistry` and lowers a parsed [`Pipeline`] into a
//! [`PhysicalPlan`]. The pushdown planner itself stays pure (no registry, no I/O); this
//! adapter feeds it owned data.
//!
//! Predicates are sourced from the AST (O-t07-3): the lowering runs over the parser
//! [`Pipeline`], not the schema-threading `PlanSource`.

use qfs_driver::{ChildAddress, Driver, Path, PushdownProfile};
use qfs_parser::{Expr, Literal, Op, PipeOp, Pipeline, Source, Statement};
use qfs_pushdown::{
    lower_query, partition_by_source, LowerError, PhysicalPlan, PlanError, SourceId, SourceRegistry,
};
use qfs_types::{ColumnType, Schema};

use crate::registry::{HostScopeError, MountRegistry};

/// A structured error from the pushdown integration: either the AST could not be lowered
/// (`LowerError`) or the partition rejected at plan time (`PlanError`). A query that does
/// not route to any mounted driver is its own arm.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PushdownError {
    /// The query AST could not be lowered into the planner IR (a non-predicate `WHERE`,
    /// an unsupported `JOIN ON`, etc.).
    Lower(LowerError),
    /// The source-split rejected at plan time (unknown source, capability denial).
    Plan(PlanError),
    /// The statement was not a pure read query, so there is nothing to push down.
    NotAQuery,
    /// A `FROM` path violated the host-realm path canon (decision P / owner ruling
    /// 2026-07-16): a non-local `/hosts/<h>/…` host, a `/hosts` with no host segment, a
    /// cross-realm service path, or a retired bare spelling of a host-realm-only mount.
    HostScope(HostScopeError),
    /// A selection segment (`/x/@A`, 番地の`@選択`) could not be lowered: the base node
    /// declares no child key, the value list mismatches the declared key columns, a value
    /// does not decode/type, or the selection is not the final segment.
    Selection(SelectionError),
}

/// Why a selection segment (`/x/@A`) refused to lower at the one lowering site.
/// Structured and stable-coded (blueprint §6) so an agent — or the viewer — can recover.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectionError {
    /// The selection is not the FINAL segment. Relation segments (`/x/@A/thread`) belong
    /// to a later phase; today a selection ends the address.
    NotFinal {
        /// The offending full path.
        path: String,
    },
    /// The base path routes to no mounted driver, so no key declaration exists to read.
    Unrouted {
        /// The unrouted base path.
        path: String,
    },
    /// The base node declares no child KEY: either [`ChildAddress::None`] (its rows select
    /// no child — not every table is a tree) or [`ChildAddress::EntryName`] (a blob child
    /// is addressed by its entry name segment, `/x/<name>`, never by `@`).
    NoChildKey {
        /// The base node path.
        path: String,
        /// What the node declared instead — the honest pointer for recovery.
        declared: ChildAddress,
    },
    /// The number of comma-separated values does not match the declared key columns.
    Arity {
        /// The base node path.
        path: String,
        /// How many key columns the node declares.
        declared: usize,
        /// How many values the selection carried.
        given: usize,
    },
    /// A value failed to percent-decode, or does not type against its declared key column.
    BadValue {
        /// The base node path.
        path: String,
        /// The raw offending value text.
        value: String,
        /// The key column it was matched against, with its declared type.
        column: String,
    },
}

impl SelectionError {
    /// A stable, machine-readable code (blueprint §6).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            SelectionError::NotFinal { .. } => "selection_not_final",
            SelectionError::Unrouted { .. } => "selection_unrouted",
            SelectionError::NoChildKey { .. } => "selection_no_child_key",
            SelectionError::Arity { .. } => "selection_arity",
            SelectionError::BadValue { .. } => "selection_bad_value",
        }
    }
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionError::NotFinal { path } => write!(
                f,
                "selection segment must be the final segment of `{path}` (relation segments are not built yet)"
            ),
            SelectionError::Unrouted { path } => {
                write!(f, "selection base `{path}` routes to no mounted driver")
            }
            SelectionError::NoChildKey { path, declared } => match declared {
                ChildAddress::EntryName { column } => write!(
                    f,
                    "`{path}` children are addressed by entry name (the `{column}` column value as a path segment), not by `@` selection"
                ),
                _ => write!(
                    f,
                    "`{path}` declares no child key; its rows select no child"
                ),
            },
            SelectionError::Arity {
                path,
                declared,
                given,
            } => write!(
                f,
                "`{path}` declares {declared} key column(s) but the selection carries {given} value(s)"
            ),
            SelectionError::BadValue {
                path,
                value,
                column,
            } => write!(
                f,
                "selection value `{value}` does not decode/type against key column {column} of `{path}`"
            ),
        }
    }
}

impl PushdownError {
    /// A stable, machine-readable code (blueprint §6).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            PushdownError::Lower(e) => e.code(),
            PushdownError::Plan(e) => e.code(),
            PushdownError::NotAQuery => "not_a_query",
            PushdownError::HostScope(e) => e.code(),
            PushdownError::Selection(e) => e.code(),
        }
    }
}

impl From<LowerError> for PushdownError {
    fn from(e: LowerError) -> Self {
        PushdownError::Lower(e)
    }
}

impl From<PlanError> for PushdownError {
    fn from(e: PlanError) -> Self {
        PushdownError::Plan(e)
    }
}

/// Build a [`qfs_pushdown::SourceRegistry`] from the live [`MountRegistry`]: each mounted
/// driver contributes its [`Driver::id`] as a [`SourceId`] and its [`Driver::pushdown`]
/// profile. The synthetic `(values)` source (inline `VALUES`) is registered as a
/// `None`-pushdown local source so a `FROM VALUES` partitions without an "unknown source"
/// error.
#[must_use]
pub fn source_registry(mounts: &MountRegistry) -> SourceRegistry {
    let mut reg = SourceRegistry::new();
    for driver in mounts.drivers() {
        reg.register(SourceId::new(driver.id().as_str()), *driver.pushdown());
    }
    reg.register(SourceId::new("(values)"), PushdownProfile::None);
    reg
}

/// Lower + partition a query [`Statement`] into a [`PhysicalPlan`] using the live mount
/// registry (ticket t14 entry point). The source of each `/driver/...` leaf is the
/// driver its first path segment routes to; its schema comes from the driver's pure
/// `describe` (no I/O). A non-query statement is [`PushdownError::NotAQuery`].
///
/// # Errors
/// [`PushdownError`] if the AST cannot be lowered or the split is rejected at plan time.
pub fn plan_query(stmt: &Statement, mounts: &MountRegistry) -> Result<PhysicalPlan, PushdownError> {
    let Statement::Query(pipeline) = stmt else {
        return Err(PushdownError::NotAQuery);
    };
    plan_pipeline(pipeline, mounts)
}

/// Lower + partition a [`Pipeline`] directly (the inner form `plan_query` delegates to).
///
/// # Errors
/// [`PushdownError`] if the AST cannot be lowered or the split is rejected at plan time.
pub fn plan_pipeline(
    pipeline: &Pipeline,
    mounts: &MountRegistry,
) -> Result<PhysicalPlan, PushdownError> {
    // The host-realm path canon (decision P / owner ruling 2026-07-16) runs BEFORE lowering, so
    // every `FROM /hosts/local/<svc>/…` leaf lowers with its peeled SERVICE path — the scan the
    // read driver later receives must speak the mount's own namespace, and the retired bare
    // spelling of a host-realm-only mount fails here with the canonical pointer.
    let mut pipeline = canonicalize_hosts(pipeline, mounts).map_err(PushdownError::HostScope)?;
    // 番地の`@選択`: THE one lowering site (plan.md「選択セグメントの綴り」, settled
    // 2026-07-18). Every read position — FROM, JOIN source, subquery, set-op branch —
    // funnels through here, so `/x/@A` lowers to `read /x |> where <declared key> == A`
    // exactly once, for every consumer (one-shot run, REPL, server, MCP, view refresh).
    lower_selections_pipeline(&mut pipeline, mounts).map_err(PushdownError::Selection)?;
    let pipeline = &pipeline;
    let source_of = |segs: &[String]| -> SourceId {
        let full = render_path(segs);
        match mounts.resolve_path(&full) {
            Some((driver, _)) => SourceId::new(driver.id().as_str()),
            // An unrouted FROM resolves to a synthetic source id (its first segment), so
            // the partitioner surfaces a structured `unknown_source` rather than panicking.
            None => SourceId::new(segs.first().cloned().unwrap_or_default()),
        }
    };
    let schema_of = |src: &SourceId| -> Schema {
        // Re-resolve the driver by id to fetch its described schema. A `(values)` or
        // unrouted source has no driver; it gets an empty (late-bound) schema.
        for driver in mounts.drivers() {
            if driver.id().as_str() == src.as_str() {
                return describe_root(driver.as_ref());
            }
        }
        Schema::empty()
    };

    // §15 (decision W): resolve a `|> transform <name>` stage against the definitions the binary
    // installed on the registry (empty on a pure/no-DB path ⇒ an unresolved transform is a
    // structured lowering error, never a silent passthrough).
    let transform_of = |name: &str| mounts.transform_defs().get(name).cloned();
    let logical = lower_query(pipeline, &source_of, &schema_of, &transform_of)?;
    let reg = source_registry(mounts);
    let physical = partition_by_source(&logical, &reg)?;
    Ok(physical)
}

/// Canonicalize the host realm in every `FROM` path of `pipeline` (decision P / §1.3; owner
/// ruling 2026-07-16): `/hosts/local/<svc>/…` peels to its service path (the general
/// `/hosts/<h>/<svc>` rule, never a per-driver special case), a non-local host fails closed
/// (`remote_host_not_executable` — the tunnel seam is not wired), and a retired bare spelling of
/// a host-realm-only mount fails with the canonical pointer. Walks every source position —
/// pipeline source, subquery, `JOIN` source, and each set-op branch. Clones only because the
/// planner takes `&Pipeline`; pipelines are small ASTs.
///
/// # Errors
/// [`HostScopeError`] per [`MountRegistry::canonicalize_host_path`].
fn canonicalize_hosts(
    pipeline: &Pipeline,
    mounts: &MountRegistry,
) -> Result<Pipeline, HostScopeError> {
    let mut p = pipeline.clone();
    canonicalize_pipeline_mut(&mut p, mounts)?;
    Ok(p)
}

fn canonicalize_pipeline_mut(
    p: &mut Pipeline,
    mounts: &MountRegistry,
) -> Result<(), HostScopeError> {
    canonicalize_source_mut(&mut p.source, mounts)?;
    for op in &mut p.ops {
        match op {
            PipeOp::Join(join) => canonicalize_source_mut(&mut join.source, mounts)?,
            PipeOp::Union(sub) | PipeOp::Except(sub) | PipeOp::Intersect(sub) => {
                canonicalize_pipeline_mut(sub, mounts)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn canonicalize_source_mut(s: &mut Source, mounts: &MountRegistry) -> Result<(), HostScopeError> {
    match s {
        Source::Path(path) => {
            let names: Vec<String> = path.segments.iter().map(|s| s.name.clone()).collect();
            let full = render_path(&names);
            let canonical = mounts.canonicalize_host_path(&full)?;
            if canonical != full {
                // The peel dropped exactly the `/hosts/<host>` realm prefix — drop the same two
                // leading segments from the AST path (their `@version` coordinates, were any
                // written, name the realm, not the service node — they go with them).
                path.segments.drain(..2);
            }
            Ok(())
        }
        Source::Subquery(inner) => canonicalize_pipeline_mut(inner, mounts),
        Source::Values(_) | Source::Name(_) => Ok(()),
    }
}

/// Lower every selection segment (`/x/@A`) in `p` into its `where` step — the ONE lowering
/// site's recursive walk, mirroring [`canonicalize_pipeline_mut`]'s coverage of read
/// positions: the pipeline source, each `JOIN` source, subqueries, and set-op branches.
///
/// The main source's predicate is prepended to the OWNING pipeline's ops (so `/x/@A |> …`
/// is byte-for-byte the plan of `/x |> where … |> …`); a `JOIN` source — which owns no op
/// chain — is wrapped into an equivalent subquery pipeline.
fn lower_selections_pipeline(
    p: &mut Pipeline,
    mounts: &MountRegistry,
) -> Result<(), SelectionError> {
    match &mut p.source {
        Source::Path(_) => {
            if let Some(predicate) = take_selection_predicate(&mut p.source, mounts)? {
                p.ops.insert(0, PipeOp::Where(predicate));
            }
        }
        Source::Subquery(inner) => lower_selections_pipeline(inner, mounts)?,
        Source::Values(_) | Source::Name(_) => {}
    }
    for op in &mut p.ops {
        match op {
            PipeOp::Join(join) => match &mut join.source {
                Source::Path(_) => {
                    if let Some(predicate) = take_selection_predicate(&mut join.source, mounts)? {
                        let base = join.source.clone();
                        join.source = Source::Subquery(Box::new(Pipeline {
                            source: base,
                            ops: vec![PipeOp::Where(predicate)],
                        }));
                    }
                }
                Source::Subquery(inner) => lower_selections_pipeline(inner, mounts)?,
                Source::Values(_) | Source::Name(_) => {}
            },
            PipeOp::Union(sub) | PipeOp::Except(sub) | PipeOp::Intersect(sub) => {
                lower_selections_pipeline(sub, mounts)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// If `source` is a path whose final segment is a selection (`@A`), strip that segment and
/// return the `where` predicate it lowers to — matching the base node's DECLARED key
/// column(s) ([`ChildAddress::Key`], read via the driver's pure `describe`; no I/O) against
/// the percent-decoded, positionally-ordered values, each typed by its declared column.
///
/// `Ok(None)` when the path carries no selection (the overwhelmingly common case).
fn take_selection_predicate(
    source: &mut Source,
    mounts: &MountRegistry,
) -> Result<Option<Expr>, SelectionError> {
    let Source::Path(path) = source else {
        return Ok(None);
    };
    let Some(sel_idx) = path.segments.iter().position(|s| s.selection) else {
        return Ok(None);
    };
    let full = render_path(
        &path
            .segments
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>(),
    );
    if sel_idx + 1 != path.segments.len() {
        return Err(SelectionError::NotFinal { path: full });
    }
    let raw = path.segments[sel_idx].name.clone();
    let base_segments: Vec<String> = path.segments[..sel_idx]
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let base = render_path(&base_segments);
    let Some((driver, sub)) = mounts.resolve_path(&base) else {
        return Err(SelectionError::Unrouted { path: base });
    };
    let vfs = format!("/{}/{}", driver.id().as_str(), sub);
    let desc = driver
        .describe(&Path::new(&vfs))
        .map_err(|_| SelectionError::Unrouted { path: base.clone() })?;
    let ChildAddress::Key { columns } = &desc.child_address else {
        return Err(SelectionError::NoChildKey {
            path: base,
            declared: desc.child_address.clone(),
        });
    };
    let values: Vec<&str> = raw.split(',').collect();
    if values.len() != columns.len() {
        return Err(SelectionError::Arity {
            path: base,
            declared: columns.len(),
            given: values.len(),
        });
    }
    let mut predicate: Option<Expr> = None;
    for (column, value) in columns.iter().zip(&values) {
        let ty = desc
            .schema
            .columns
            .iter()
            .find(|c| &c.name == column)
            .map(|c| &c.ty);
        let lit = key_literal(value, ty).ok_or_else(|| SelectionError::BadValue {
            path: base.clone(),
            value: (*value).to_string(),
            column: match ty {
                Some(ty) => format!("`{column}` ({ty:?})"),
                None => format!("`{column}` (undeclared in schema)"),
            },
        })?;
        let cmp = Expr::Binary {
            op: Op::Eq,
            lhs: Box::new(Expr::Col(column.clone())),
            rhs: Box::new(Expr::Lit(lit)),
        };
        predicate = Some(match predicate {
            None => cmp,
            Some(acc) => Expr::Binary {
                op: Op::And,
                lhs: Box::new(acc),
                rhs: Box::new(cmp),
            },
        });
    }
    // At least one key column exists (`NodeDesc::child_key` refuses an empty declaration),
    // so `predicate` is Some; a defensively-empty declaration still errors as arity above
    // (0 declared vs ≥1 given — `split` never yields zero items).
    let Some(predicate) = predicate else {
        return Err(SelectionError::NoChildKey {
            path: base,
            declared: ChildAddress::None,
        });
    };
    // Drop the selection segment: the scan addresses the BASE node.
    path.segments.truncate(sel_idx);
    Ok(Some(predicate))
}

/// Percent-decode a selection value and type it by the declared key column: an `Int`/
/// `Float` column takes a numeric literal, a `Bool` column `true`/`false`, anything else
/// (including a column the schema does not declare — late-bound schemas exist) a string.
/// `None` on a bad percent escape or a value that does not parse as the declared type.
fn key_literal(raw: &str, ty: Option<&ColumnType>) -> Option<Literal> {
    let decoded = percent_decode(raw)?;
    match ty {
        Some(ColumnType::Int) => decoded.parse::<i64>().ok().map(Literal::Int),
        Some(ColumnType::Float) => decoded.parse::<f64>().ok().map(Literal::Float),
        Some(ColumnType::Bool) => match decoded.as_str() {
            "true" => Some(Literal::Bool(true)),
            "false" => Some(Literal::Bool(false)),
            _ => None,
        },
        _ => Some(Literal::Str(decoded)),
    }
}

/// RFC 3986 percent-decoding of a selection value (`%2C` → `,`, `%40` → `@`). Strict:
/// a `%` not followed by two hex digits, or a decode to invalid UTF-8, is `None` — a
/// refusal, never a silently different key.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).copied()?;
            let lo = bytes.get(i + 2).copied()?;
            let hex = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            out.push(hex(hi)? * 16 + hex(lo)?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Describe a driver's root node schema via its **pure** `describe` (no I/O). An
/// undescribable root degrades to an empty (late-bound) schema, keeping lowering total.
fn describe_root(driver: &dyn Driver) -> Schema {
    let root = format!("/{}", driver.id().as_str());
    driver
        .describe(&Path::new(root))
        .map(|d| d.schema)
        .unwrap_or_else(|_| Schema::empty())
}

/// Render a logical segment list into a `/seg/seg` router path.
fn render_path(segments: &[String]) -> String {
    if segments.is_empty() {
        return "/".to_string();
    }
    let mut s = String::new();
    for seg in segments {
        s.push('/');
        s.push_str(seg);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_driver::{
        AliasFn, Archetype, Capabilities, CfsError, ChildAddress, NodeDesc, ProcSig, VersionSupport,
    };
    use qfs_parser::parse_statement;
    use qfs_plan::PlanApplier;
    use qfs_pushdown::explain;
    use qfs_types::{Column, ColumnType};
    use std::sync::Arc;

    struct FakeDriver {
        mount: String,
        pushdown: PushdownProfile,
        child: ChildAddress,
        applier: NoopApplier,
    }

    #[derive(Default)]
    struct NoopApplier;
    impl PlanApplier for NoopApplier {
        fn apply(
            &mut self,
            node: &qfs_plan::EffectNode,
        ) -> Result<qfs_plan::AppliedEffect, qfs_plan::ApplyError> {
            Ok(qfs_plan::AppliedEffect::new(node.id, 0))
        }
    }

    impl FakeDriver {
        fn new(mount: &str, pushdown: PushdownProfile) -> Self {
            Self::with_child(
                mount,
                pushdown,
                ChildAddress::Key {
                    columns: vec!["id".to_string()],
                },
            )
        }

        fn with_child(mount: &str, pushdown: PushdownProfile, child: ChildAddress) -> Self {
            Self {
                mount: mount.to_string(),
                pushdown,
                child,
                applier: NoopApplier,
            }
        }
    }

    impl Driver for FakeDriver {
        fn mount(&self) -> &str {
            &self.mount
        }
        fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
            let mut desc = NodeDesc::new(
                Archetype::RelationalTable,
                Schema::new(vec![
                    Column::new("id", ColumnType::Int, false),
                    Column::new("name", ColumnType::Text, true),
                ]),
            );
            desc.child_address = self.child.clone();
            Ok(desc)
        }
        fn capabilities(&self, _path: &Path) -> Capabilities {
            Capabilities::none().select()
        }
        fn procedures(&self) -> &[ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &self.pushdown
        }
        fn prelude(&self) -> &[AliasFn] {
            &[]
        }
        fn version_support(&self, _path: &Path) -> VersionSupport {
            VersionSupport::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            &self.applier
        }
    }

    fn registry() -> MountRegistry {
        let mut reg = MountRegistry::new();
        reg.register(Arc::new(FakeDriver::new("/db", PushdownProfile::Full)))
            .unwrap();
        reg.register(Arc::new(FakeDriver::new("/git", PushdownProfile::None)))
            .unwrap();
        reg
    }

    #[test]
    fn single_source_query_lowers_to_one_scan_via_registry() {
        let mounts = registry();
        let stmt = parse_statement("/db/users |> WHERE id > 0 |> SELECT id").unwrap();
        let phys = plan_query(&stmt, &mounts).unwrap();
        assert_eq!(phys.scan_count(), 1);
        // /db is Full → everything pushes.
        assert_eq!(explain(&phys), "Scan[db] pushed=[where, project(id)]\n");
    }

    #[test]
    fn cross_source_join_via_registry_federates() {
        let mounts = registry();
        let stmt = parse_statement("/db/users |> JOIN /git/commits ON id == id").unwrap();
        let phys = plan_query(&stmt, &mounts).unwrap();
        assert_eq!(phys.scan_count(), 2);
        // /db pushes nothing extra (bare scan), /git is None (bare). The join federates.
        assert!(explain(&phys).starts_with("Combine[HashJoin(id = id)]"));
    }

    #[test]
    fn none_source_leaves_residual_local_via_registry() {
        let mounts = registry();
        let stmt = parse_statement("/git/log |> WHERE id > 0").unwrap();
        let phys = plan_query(&stmt, &mounts).unwrap();
        // /git is None → the WHERE is a local residual.
        assert_eq!(explain(&phys), "Combine[Filter]\n  Scan[git] pushed=[]\n");
    }

    #[test]
    fn non_query_statement_is_rejected() {
        let mounts = registry();
        let stmt = parse_statement("INSERT INTO /db/users VALUES (1, 'a')").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "not_a_query");
    }

    /// The host-realm path canon in planning (decision P / owner ruling 2026-07-16): a
    /// `/hosts/local/<svc>/…` FROM peels to the service path — the ScanNode the read driver
    /// receives speaks the mount's own namespace — and the peel is the GENERAL `/hosts/<h>/<svc>`
    /// rule (proven over the ordinary `/db` fake, not a claude special-case).
    #[test]
    fn hosts_local_from_plans_as_the_peeled_service_scan() {
        let mounts = registry();
        let stmt = parse_statement("/hosts/local/db/users |> WHERE id > 0 |> SELECT id").unwrap();
        let phys = plan_query(&stmt, &mounts).unwrap();
        assert_eq!(phys.scan_count(), 1);
        assert_eq!(explain(&phys), "Scan[db] pushed=[where, project(id)]\n");
        // The scan's addressed VFS path is the PEELED service path (what a read driver parses).
        assert_eq!(phys.scans()[0].path, "/db/users");
    }

    /// A non-local host fails closed at plan time with the structured remote error (the tunnel
    /// seam is not wired), and `/hosts` with no host segment is a missing principal.
    #[test]
    fn hosts_remote_and_missing_principal_fail_closed_at_plan_time() {
        let mounts = registry();
        let stmt = parse_statement("/hosts/qfs.cloud/db/users |> LIMIT 1").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "remote_host_not_executable");
        assert!(matches!(&err, PushdownError::HostScope(h) if h.to_string().contains("qfs.cloud")));

        let stmt = parse_statement("/hosts |> LIMIT 1").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "missing_principal");
    }

    /// The retired bare spelling of a host-realm-only mount fails at plan time with the
    /// canonical pointer — and the check reaches a JOIN source, not only the FROM position.
    #[test]
    fn retired_bare_path_fails_with_the_canonical_pointer() {
        let mut mounts = registry();
        mounts.require_host_realm("/db");

        let stmt = parse_statement("/db/users |> LIMIT 1").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "retired_path");
        assert!(
            matches!(&err, PushdownError::HostScope(h) if h.to_string().contains("/hosts/local/db/users"))
        );

        // The canonical spelling still plans (the peel, not a lockout).
        let stmt = parse_statement("/hosts/local/db/users |> LIMIT 1").unwrap();
        assert_eq!(plan_query(&stmt, &mounts).unwrap().scan_count(), 1);

        // A JOIN source is walked too.
        let stmt = parse_statement("/git/commits |> JOIN /db/users ON id == id").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "retired_path");
    }

    // ---- selection segments (番地の`@選択`) — the ONE lowering site --------------------

    /// `/x/@A` lowers deterministically to `read /x |> where <declared key> == A` — the
    /// plan is IDENTICAL to the spelled-out where, down to the pushed predicate, and the
    /// scan's addressed path is the BASE node (the selection never reaches a driver as a
    /// containment name).
    #[test]
    fn selection_segment_lowers_to_the_declared_key_where() {
        let mounts = registry();
        let sel = parse_statement("/db/users/@7 |> SELECT id").unwrap();
        let spelled = parse_statement("/db/users |> WHERE id == 7 |> SELECT id").unwrap();
        let got = plan_query(&sel, &mounts).unwrap();
        let want = plan_query(&spelled, &mounts).unwrap();
        assert_eq!(explain(&got), explain(&want), "one lowering, same plan");
        assert_eq!(explain(&got), "Scan[db] pushed=[where, project(id)]\n");
        assert_eq!(
            got.scans()[0].path,
            "/db/users",
            "the scan addresses the BASE"
        );
        assert_eq!(
            format!("{:?}", got.scans()[0].pushed),
            format!("{:?}", want.scans()[0].pushed),
            "the pushed predicate is the spelled where, typed by the described key column"
        );
    }

    /// Composite keys are positional in declared key order (`@<v1>,<v2>`), values
    /// percent-decoded per RFC 3986 (`%2C` → a literal comma inside a value), each typed by
    /// its declared column.
    #[test]
    fn composite_selection_decodes_positionally_in_declared_key_order() {
        let mut mounts = MountRegistry::new();
        mounts
            .register(Arc::new(FakeDriver::with_child(
                "/crm",
                PushdownProfile::Full,
                ChildAddress::Key {
                    columns: vec!["id".to_string(), "name".to_string()],
                },
            )))
            .unwrap();
        let sel = parse_statement("/crm/invoices/@7,INV%2C003").unwrap();
        let spelled =
            parse_statement("/crm/invoices |> WHERE id == 7 AND name == 'INV,003'").unwrap();
        let got = plan_query(&sel, &mounts).unwrap();
        let want = plan_query(&spelled, &mounts).unwrap();
        assert_eq!(
            format!("{:?}", got.scans()[0].pushed),
            format!("{:?}", want.scans()[0].pushed)
        );
        assert_eq!(got.scans()[0].path, "/crm/invoices");
    }

    /// A selection in a JOIN source lowers through the same site — every read position
    /// funnels through `plan_pipeline`, so there is no second gatekeeper to drift.
    #[test]
    fn selection_in_a_join_source_lowers_too() {
        let mounts = registry();
        let stmt = parse_statement("/db/users |> JOIN /git/commits/@7 ON id == id").unwrap();
        let phys = plan_query(&stmt, &mounts).unwrap();
        assert_eq!(phys.scan_count(), 2);
        let paths: Vec<&str> = phys.scans().iter().map(|s| s.path.as_str()).collect();
        assert!(
            paths.contains(&"/git/commits"),
            "join-side scan addresses the base, got {paths:?}"
        );
    }

    /// A node that declares NO child key ([`ChildAddress::None`] — a `{value}` table — or
    /// the blob [`ChildAddress::EntryName`], whose children are name segments, not `@`
    /// selections) refuses the selection segment with a structured error.
    #[test]
    fn selection_against_a_keyless_node_is_a_structured_refusal() {
        let mut mounts = MountRegistry::new();
        mounts
            .register(Arc::new(FakeDriver::with_child(
                "/flat",
                PushdownProfile::Full,
                ChildAddress::None,
            )))
            .unwrap();
        mounts
            .register(Arc::new(FakeDriver::with_child(
                "/blob",
                PushdownProfile::Full,
                ChildAddress::EntryName {
                    column: "name".to_string(),
                },
            )))
            .unwrap();
        let stmt = parse_statement("/flat/rows/@7").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "selection_no_child_key");
        let stmt = parse_statement("/blob/dir/@x").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "selection_no_child_key");
        assert!(
            matches!(&err, PushdownError::Selection(s) if s.to_string().contains("entry name")),
            "the blob refusal points at name-segment addressing; got: {err:?}"
        );
    }

    /// Malformed selections refuse loudly: a non-final selection segment (the relation
    /// phase is not built), a value-count/key-count mismatch, and a value that does not
    /// decode or type against its declared column.
    #[test]
    fn malformed_selections_are_structured_errors() {
        let mounts = registry();
        // Not the final segment — relation segments are a later phase.
        let stmt = parse_statement("/db/users/@7/more").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "selection_not_final");
        // Arity: one declared key column, two values.
        let stmt = parse_statement("/db/users/@1,2").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "selection_arity");
        // Bad percent-encoding.
        let stmt = parse_statement("/db/users/@%GG").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "selection_bad_value");
        // The declared key column `id` is Int; a non-numeric value cannot select by it.
        let stmt = parse_statement("/db/users/@abc").unwrap();
        let err = plan_query(&stmt, &mounts).unwrap_err();
        assert_eq!(err.code(), "selection_bad_value");
    }
}
