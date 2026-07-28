//! `qfs-engine` — the **local combine engine** (blueprint §7, ticket t14, ADR-0002).
//!
//! The pushdown planner (`qfs-pushdown`) splits a query into native [`ScanNode`]s the
//! drivers run themselves plus a local residual ([`PhysicalPlan::Combine`] ops). This
//! crate is the seam that **executes that residual** over the scan results: filter,
//! project, hash-join, set ops, group/aggregate, sort, limit, EXPAND.
//!
//! ## The engine decision (ADR-0002): own [`MiniEvaluator`], not embedded DuckDB
//! The DuckDB-vs-own question blueprint §7 flags is resolved in blueprint §11:
//! we ship an in-house [`MiniEvaluator`] behind the [`CombineEngine`] trait. DuckDB
//! cannot build to `wasm32-unknown-unknown` (the Cloudflare Workers target, blueprint §1/§11)
//! and adds a large static C++ footprint, contradicting the "single binary / wasm32 / no
//! heavy vendor SDK" rule — while the residual is only ever a **small** relational subset
//! (the heavy lifting is pushed down). The trait keeps the choice reversible: an optional
//! `DuckDbEngine` could land behind a non-default feature without touching callers.
//!
//! ## Correctness contract (the differential property)
//! [`MiniEvaluator::execute`] over a [`PhysicalPlan`] returns exactly the rows a naive
//! all-local evaluation of the original query would (`qfs-pushdown` guarantees the split
//! is semantically total; this crate's tests assert push-then-combine == all-local).
//!
//! ## Purity / wasm-friendliness
//! Dependency-light (only `qfs-pushdown` + `qfs-types`): no I/O, no async, no threads, no
//! vendor SDK. The engine computes over owned [`RowBatch`]es a (future) driver produced;
//! it never opens a socket itself.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod combine;
mod eval;
mod scan;

pub use combine::{CombineEngine, EngineError, MiniEvaluator, TransformCall, TransformExecutor};
pub use eval::eval_value;
pub use scan::ScanResults;

/// Re-filter a [`RowBatch`](qfs_types::RowBatch) by a residual [`Predicate`](qfs_types::Predicate)
/// — the read-facet residual seam. A driver that pushed only PART of a `WHERE` into its native query
/// returns the (over-returned) rows plus the predicate it could not faithfully render; the caller
/// applies it here so the rows are exactly the pushed query's result before the engine runs the
/// remaining cross-source residual. Total: an incomparable / late-bound comparison drops the row
/// (the same semantics the [`MiniEvaluator`]'s own `WHERE` residual uses, so push-then-filter equals
/// all-local).
#[must_use]
pub fn apply_residual(
    batch: qfs_types::RowBatch,
    predicate: &qfs_types::Predicate,
) -> qfs_types::RowBatch {
    eval::filter(batch, predicate)
}

/// Enforce a **caller-written `WHERE`** over a [`RowBatch`](qfs_types::RowBatch) — the read
/// executor's pushed-predicate seam. Same evaluation as [`apply_residual`], plus the refusal
/// [`apply_residual`] must not make: a predicate naming a column the (described, non-empty) schema
/// does not carry is [`EngineError::UnknownColumn`], not an empty relation at exit 0.
///
/// The split is deliberate. A **driver's** residual may name a backend search pseudo-column the
/// schema does not carry (Drive's `fullText`, Gmail's `to`) and must still evaluate; a **caller's**
/// `WHERE` naming a column that is not there is a malformed question, and answering "none" to it
/// corrupts every consumer that branches on `row_count`.
///
/// # Errors
/// [`EngineError::UnknownColumn`] when the predicate names an absent column.
pub fn apply_where(
    batch: qfs_types::RowBatch,
    predicate: &qfs_types::Predicate,
) -> Result<qfs_types::RowBatch, EngineError> {
    eval::filter_checked(batch, predicate)
        .map_err(|missing| EngineError::unknown_column("where", missing))
}

/// Refuse a caller-written `WHERE` that names a column neither the relation's (described) schema
/// nor `also_accepted` carries — the check without the filtering, for a source that resolves the
/// predicate itself.
///
/// A facet that declares `honors_pushed_filter` translates the `WHERE` into its backend's own
/// query language and never routes it through [`apply_where`], so this is where it gets the same
/// refusal. `also_accepted` is that backend's **search pseudo-columns** — names it genuinely
/// filters on that no row carries (Drive's `fullText`/`parent`, Gmail's `label`/`is_unread`) —
/// which must not be mistaken for typos.
///
/// # Errors
/// [`EngineError::UnknownColumn`] when the predicate names an unaccepted column.
pub fn check_where_columns(
    schema: &qfs_types::Schema,
    predicate: &qfs_types::Predicate,
    also_accepted: &[&str],
) -> Result<(), EngineError> {
    eval::check_predicate_columns(schema, predicate, also_accepted)
        .map_err(|missing| EngineError::unknown_column("where", missing))
}
