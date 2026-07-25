//! The **read seam** (`ReadDriver`) — the read counterpart of the runtime's write
//! [`ApplyDriver`](qfs_runtime). t20 carry-over closure.
//!
//! ## Why a new seam (the t20 carry-over)
//! Before t29 there was no SELECT read-path execution. The runtime's `ApplyDriver` is a
//! *write/commit* batching engine whose `EffectOutput` carries only an affected **count**
//! — it structurally cannot return [`RowBatch`] rows, so it is not a read seam. Each E4
//! driver grew its own pure `ReadPlan`/`plan_read` (a self-describing description of what to
//! fetch), but **nothing executed those and fed the rows into the engine**. This trait is
//! that execution seam: given one native [`ScanNode`] (source + the pushed predicate/
//! projection/limit + resolved schema), the driver runs its own I/O and returns an owned
//! [`RowBatch`] — exactly the unit [`qfs_engine::ScanResults`] consumes positionally.
//!
//! ## Async, owned, vendor-free (blueprint §11)
//! Scans are I/O, so the seam is `async` (object-safe via `async-trait`, like `ApplyDriver`).
//! The driver translates the owned [`PushedQuery`](qfs_pushdown::PushedQuery) inside its own
//! boundary; **no vendor SDK type crosses this seam** — only the owned [`ScanNode`] in and
//! the owned [`RowBatch`] out. Tests register an in-memory fake (no creds, no network).
//!
//! ## Pushdown honesty + the residual property (t20)
//! A driver MAY honestly under-apply the pushed work (e.g. return more rows than `LIMIT`,
//! or ignore a `WHERE` it cannot run): the executor re-applies the **residual** locally via
//! the [`MiniEvaluator`](qfs_engine::MiniEvaluator). The seam therefore only promises the
//! driver returns a superset of the rows the pushed query selects; correctness is restored
//! by the engine's residual re-filter over the over-returned rows.
//!
//! ## The pushed `WHERE` is enforced, never delegated on trust
//! That promise is now **structural**: [`crate::execute_read`] re-applies `scan.pushed.filter`
//! over every batch a facet returns, unless the facet declares
//! [`ReadDriver::honors_pushed_filter`]. A facet that ignores the pushed predicate therefore
//! over-returns (allowed) instead of answering an unfiltered relation as if it had matched
//! (a wrong answer at exit 0). The declaration exists only for the facets that CANNOT be
//! re-checked from outside — those that narrow the row shape to a pushed projection, or that
//! accept backend search pseudo-columns the described schema does not carry — and each of
//! those applies its own truthful residual inside the facet.

use std::collections::HashMap;
use std::sync::Arc;

use qfs_core::{CfsError, DriverId, RequestContext, RowBatch};
use qfs_pushdown::ScanNode;

/// The async read seam a source implements to execute one native [`ScanNode`] and return
/// its rows. The read counterpart of the runtime's write `ApplyDriver`.
///
/// `Send + Sync` so the [`ReadRegistry`] can hold `Arc<dyn ReadDriver>` and the executor can
/// run scans concurrently under tokio.
#[async_trait::async_trait]
pub trait ReadDriver: Send + Sync {
    /// Execute one native scan **under the request's principal** (`ctx`), returning the owned rows.
    ///
    /// The driver runs the work described by `scan.pushed` against `scan.source`/`scan.schema`
    /// and returns at least the rows that work selects (it may return a superset; the executor
    /// re-applies the residual). `ctx` carries *who is asking* (the M2 principal seam) — most
    /// drivers ignore it, but a principal-aware face (e.g. `/sys/whoami`) reads it back. The seam
    /// is explicit so fail-closed is a compile-time fact: a driver cannot forget the actor exists.
    /// Returns a structured [`CfsError`] on an I/O / decode failure.
    ///
    /// # Errors
    /// [`CfsError`] if the scan could not be executed (auth, I/O, decode).
    async fn scan(&self, scan: &ScanNode, ctx: &RequestContext) -> Result<RowBatch, CfsError>;

    /// Whether the rows this facet returns **already satisfy** `scan.pushed.filter` exactly, so
    /// the executor must not re-apply it.
    ///
    /// The default is `false`: the executor re-filters, and a facet that ignores the pushed
    /// predicate merely over-returns instead of silently answering the unfiltered relation. That
    /// default is the guarantee — a `WHERE` is honored or refused, never dropped — and it holds
    /// for every facet that never states otherwise, including future and declared drivers.
    ///
    /// Override with `true` ONLY when the facet cannot be re-checked from outside, and then only
    /// because it enforces the predicate itself:
    ///
    /// - it narrows the batch to a pushed **projection**, so the predicate's columns may no longer
    ///   be present to re-check (`/sql`, `/cf` D1, `/ga`), or
    /// - it accepts backend **search pseudo-columns** the described schema does not carry
    ///   (`label:` / `is:unread` for `/mail`, `fullText contains` for `/drive`), or literals it
    ///   coerces before comparing.
    ///
    /// Every such facet applies its own driver-computed **truthful residual** — the part the
    /// backend could not express exactly — over the fetched rows. Returning `true` without doing
    /// that reintroduces the silent-drop defect this seam exists to prevent.
    fn honors_pushed_filter(&self) -> bool {
        false
    }
}

/// The read-time driver registry: maps a [`DriverId`] to the [`ReadDriver`] that services its
/// scans. The executor resolves each `ScanNode.source` through this. Separate from the
/// introspective `MountRegistry` (pure `describe`/`pushdown`) and the runtime's write
/// `DriverRegistry`: a real E4 driver registers all three facets; tests register only the
/// read facet they exercise.
#[derive(Clone, Default)]
pub struct ReadRegistry {
    drivers: HashMap<DriverId, Arc<dyn ReadDriver>>,
}

impl ReadRegistry {
    /// An empty read registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the read driver for `id` (builder form).
    #[must_use]
    pub fn with(mut self, id: DriverId, driver: Arc<dyn ReadDriver>) -> Self {
        self.drivers.insert(id, driver);
        self
    }

    /// Register the read driver for `id` (mutating form).
    pub fn register(&mut self, id: DriverId, driver: Arc<dyn ReadDriver>) {
        self.drivers.insert(id, driver);
    }

    /// Resolve the read driver for `id`, if registered.
    #[must_use]
    pub fn get(&self, id: &DriverId) -> Option<Arc<dyn ReadDriver>> {
        self.drivers.get(id).cloned()
    }
}

impl std::fmt::Debug for ReadRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadRegistry")
            .field("drivers", &self.drivers.keys().collect::<Vec<_>>())
            .finish()
    }
}
