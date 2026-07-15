//! blueprint §13 **tier 2** — the binary-side glue for declared-view body evaluation. The execution
//! logic (rehydrate, confine, run the body's ops through the engine, shape to the `OF` type) lives in
//! [`qfs_exec::declared`] — kept off the binary so the binary stays off the lower spine
//! (`qfs-parser`/`qfs-engine`); the binary only builds the per-view specs (which need the
//! binary-local [`crate::declared_driver::DeclaredDriver`] model) and injects the driver-specific
//! wire fetch as a closure (see [`crate::read_facets::RestReadDriver`]).

/// Build the [`qfs_exec::declared::ViewSpec`]s for a declared driver: each view's mount-path
/// template, its stored body, and its `OF`-type column names resolved against the declared types.
pub(crate) fn view_specs(
    d: &crate::declared_driver::DeclaredDriver,
    types: &[crate::declared_driver::DeclaredType],
) -> Vec<qfs_exec::declared::ViewSpec> {
    d.views
        .iter()
        .map(|v| {
            // Resolve the declared `OF` type once — both its column names and its §5.4 refinement
            // predicate ride together into the spec (the delivered contract, columns + membership).
            // A declared type that does NOT resolve (no row, or a stale pre-§5.4 body that parses
            // to zero columns) deliberately yields `Some(vec![])`: `eval_view_body` refuses that
            // loudly at read time (ticket 20260712005100) — never a silent zero-column projection.
            let of_type = v
                .of_type
                .as_deref()
                .and_then(|t| types.iter().find(|dt| dt.path == t));
            qfs_exec::declared::ViewSpec {
                template: v.path.clone(),
                body: v.body.clone(),
                of_columns: v
                    .of_type
                    .as_deref()
                    .map(|_| of_type.map(|dt| dt.columns.clone()).unwrap_or_default()),
                of_refinement: of_type.and_then(|dt| dt.refinement.clone()),
            }
        })
        .collect()
}

/// Build the [`qfs_exec::declared::MapSpec`]s for a declared driver: each map's mount-path template
/// and its stored effect body (the `VALUES (<expr>)` wire mapping the write facet evaluates per
/// incoming row). The write twin of [`view_specs`].
pub(crate) fn map_specs(
    d: &crate::declared_driver::DeclaredDriver,
) -> Vec<qfs_exec::declared::MapSpec> {
    d.maps
        .iter()
        .map(|m| qfs_exec::declared::MapSpec {
            template: m.path.clone(),
            body: m.body.clone(),
        })
        .collect()
}
