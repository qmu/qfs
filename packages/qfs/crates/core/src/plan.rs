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

use qfs_driver::{Driver, Path, PushdownProfile};
use qfs_parser::{PipeOp, Pipeline, Source, Statement};
use qfs_pushdown::{
    lower_query, partition_by_source, LowerError, PhysicalPlan, PlanError, SourceId, SourceRegistry,
};
use qfs_types::Schema;

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
    let pipeline = canonicalize_hosts(pipeline, mounts).map_err(PushdownError::HostScope)?;
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
        AliasFn, Archetype, Capabilities, CfsError, NodeDesc, ProcSig, VersionSupport,
    };
    use qfs_parser::parse_statement;
    use qfs_plan::PlanApplier;
    use qfs_pushdown::explain;
    use qfs_types::{Column, ColumnType};
    use std::sync::Arc;

    struct FakeDriver {
        mount: String,
        pushdown: PushdownProfile,
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
            Self {
                mount: mount.to_string(),
                pushdown,
                applier: NoopApplier,
            }
        }
    }

    impl Driver for FakeDriver {
        fn mount(&self) -> &str {
            &self.mount
        }
        fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(
                Archetype::RelationalTable,
                Schema::new(vec![
                    Column::new("id", ColumnType::Int, false),
                    Column::new("name", ColumnType::Text, true),
                ]),
            ))
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
}
