//! The **dispatching applier** (blueprint §16, the amended transport ruling): one
//! [`PlanApplier`] that routes a mixed reconcile batch to its two store appliers.
//!
//! `commit()` takes exactly ONE applier, and each store applier refuses foreign nodes
//! ([`ServerConfigApplier`] rejects non-`ServerConfigWrite` effects; the `/sys` applier rejects
//! non-`/sys` paths). [`ReconcileApplier`] is the thin **router** the ticket specifies — not a
//! third applier: an [`EffectKind::ServerConfigWrite`] node goes to the [`ServerConfigApplier`]
//! (the same seam boot and a live `/server` write use), a `/sys`-addressed node goes to the
//! injected sys applier `S`, and anything else is a structured refusal — the reconcile batch can
//! never write outside the two config stores.
//!
//! ## Why `S` is generic (dep-direction, decision F)
//! The concrete `/sys` applier (`qfs-driver-sys`'s `SysApplier`) is a **`qfs-runtime` consumer**,
//! and the leaf-confinement guard forbids any non-terminal crate depending onto such a consumer.
//! So this crate does NOT name it: the router is generic over any [`PlanApplier`], and the
//! terminal binary — the leaf where the runtime dead-ends — injects the real `SysApplier`.

use std::sync::{Arc, RwLock};

use qfs_core::{AppliedEffect, ApplyError, EffectKind, EffectNode, PlanApplier};
use qfs_server::{ConfigChange, ServerConfigApplier, ServerState};

/// The `/sys` mount prefix a reconcile op addresses (path routing — the crate stays off
/// `qfs-driver-sys` so it does not pull the runtime toward the spine).
const SYS_PREFIX: &str = "/sys/";

/// The reconcile batch router: `/server` effects to the real [`ServerConfigApplier`], `/sys`
/// effects to the injected applier `S`, anything else refused. Holds the server applier's
/// collected [`ConfigChange`]s so the caller can audit + reconcile bindings after the commit,
/// exactly as the runtime does.
pub struct ReconcileApplier<'s, S: PlanApplier> {
    server: ServerConfigApplier<'s>,
    sys: S,
}

impl<'s, S: PlanApplier> ReconcileApplier<'s, S> {
    /// Build the router over the shared `ServerState` handle (the daemon's live registry, or a
    /// fresh one in tests) and the injected `/sys` applier.
    pub fn new(state: &'s Arc<RwLock<ServerState>>, sys: S) -> Self {
        Self {
            server: ServerConfigApplier::new(state),
            sys,
        }
    }

    /// The `/server` config changes applied through this router, in apply order (consumes the
    /// router) — what the caller feeds the audit ledger and `reconcile_all()`.
    pub fn into_changes(self) -> Vec<ConfigChange> {
        self.server.into_changes()
    }
}

impl<S: PlanApplier> PlanApplier for ReconcileApplier<'_, S> {
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        match &node.kind {
            // The /server half: the same COMMIT applier boot and live writes use.
            EffectKind::ServerConfigWrite { .. } => self.server.apply(node),
            // The /sys half: any effect addressed under the /sys mount routes to the sys applier
            // (which gates verbs per node — belt and suspenders).
            _ if node.target.path.as_str().starts_with(SYS_PREFIX) => self.sys.apply(node),
            // A reconcile batch contains only the two stores; anything else is a wiring bug.
            other => Err(ApplyError::new(
                node.id,
                format!(
                    "reconcile applier received an effect outside the /server + /sys \
                     universe ({})",
                    other.label()
                ),
            )),
        }
    }
}
