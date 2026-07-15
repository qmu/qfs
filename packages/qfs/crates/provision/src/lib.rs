//! qfs **declarative provisioning** — the pure fetch → diff → reconcile core (blueprint §16,
//! Decision X).
//!
//! This crate is the store-agnostic half of `qfs plan` / `qfs apply` (increments 1+2): no CLI
//! verbs and no daemon transport yet, but the whole two-store universe. It provides five
//! matched pieces over a [`ConfigState`] (the `/server` [`qfs_server::ServerState`] plus the
//! `/sys` [`SysState`]):
//!
//! 1. [`emit`] — render both stores as the canonical `.qfs` "as code" source-of-truth document,
//!    deterministic and **config-projection only** (runtime freshness fields and secretish
//!    settings never appear; billing and `sys_ddl_events` are outside the universe entirely).
//! 2. [`load`] — parse such a document back into a desired state: the `/server` half through
//!    the exact lower/apply seam boot uses (CREATE ≡ INSERT), the `/sys` half decoded from the
//!    write twins (and the `CONNECT` sugar, which the parser itself desugars to the same write).
//! 3. [`diff`] — reduce desired-vs-current to a Terraform-style [`ReconcilePlan`] of
//!    add/change/destroy [`ReconcileOp`]s, equality decided on the config projection so
//!    cosmetic formatting and view refreshes are **not** drift. Policies key per store.
//! 4. [`build_plan`] — render a [`ReconcilePlan`] as one batch [`qfs_core::Plan`], destroys
//!    flagged irreversible so `preview()` and the `IrreversibleGuard` see them.
//! 5. [`ReconcileApplier`] — the dispatching applier: one `PlanApplier` routing
//!    `ServerConfigWrite` nodes to the real [`qfs_server::ServerConfigApplier`] and `/sys`
//!    nodes to a **generic** injected sys applier (the terminal binary supplies the concrete
//!    `SysApplier`; this crate stays off the runtime-consuming driver, decision F).
//!
//! Increment 3 adds the CLI verbs, the daemon statement-face transport (host-not-serving
//! refusal, boot-config re-emission), and the stale-base gate.

// Test code is exempted from the strict no-unwrap/expect/panic library policy, per the
// per-crate convention (CLAUDE.md build & test).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod diff;
mod dispatch;
mod emit;
mod load;
mod plan;
mod proj;
mod state;

#[cfg(test)]
mod tests;

pub use diff::{diff, ConfigStore, ReconcileNode, ReconcileOp, ReconcilePlan};
pub use dispatch::ReconcileApplier;
pub use emit::{emit, server_op_statement, DdlEventHead, GenerationStamp};
pub use load::{load, LoadError};
pub use plan::build_plan;
pub use proj::{
    endpoint_proj, job_proj, name_only, policy_proj, trigger_proj, view_proj, webhook_proj, ProjRow,
};
pub use state::{
    path_binding_proj, sys_driver_proj, sys_policy_proj, sys_setting_proj, sys_transform_proj,
    ConfigState, PathBindingRow, SysCollection, SysDriverRow, SysPolicyRow, SysState, TransformRow,
};

// Re-export the owned `/server` config DTOs so a consumer (the terminal binary) can construct the
// shared `ServerState` the `ReconcileApplier` holds WITHOUT a direct `qfs-server` edge (the
// thin-entrypoint guard forbids the binary depending directly on the lower spine).
pub use qfs_server::{
    EndpointDef, JobDef, JobRunRecord, PolicyDef, ServerState, StatementSource, TriggerDef,
    ViewDef, WebhookDef,
};
