//! The **read-only-by-default policy gate** (t32, RFD §3 purity / §10 least-privilege).
//!
//! An endpoint serves the *query face* of cfs, not effects. [`assert_read_only`] walks a
//! lowered [`cfs_core::Plan`] for write [`cfs_core::EffectKind`]s and **default-denies** any
//! write — an endpoint whose query lowers to a Plan containing a write effect is REFUSED
//! unless a [`cfs_server::PolicyDef`] grants it. It is enforced at BOTH:
//!   * **route compile** (registration): a write-lowering endpoint never becomes a live route
//!     (the plan-assertion acceptance);
//!   * **request time** (defence in depth): even a hot-swapped route re-asserts before eval.
//!
//! The full POLICY capability engine is t34; this is the registration-time gate + the hook a
//! [`PolicyDef`] plugs into. A pure read query (`FROM … |> WHERE …`) lowers to a plan with no
//! effect nodes, so it passes unconditionally.

use cfs_core::{EffectKind, Plan};
use cfs_server::PolicyDef;

/// A read-only-policy denial: the endpoint's query lowers to a write effect and no policy
/// grants it. Maps to HTTP 403. Secret-free — names only the effect label, never any data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyError {
    /// The stable label of the first write effect found (e.g. `INSERT`, `REMOVE`).
    pub effect: String,
}

impl PolicyError {
    /// Construct a denial naming the offending write effect.
    #[must_use]
    pub fn new(effect: impl Into<String>) -> Self {
        Self {
            effect: effect.into(),
        }
    }
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "endpoint query performs a write effect ({}) but is read-only by default; no \
             POLICY grants it",
            self.effect
        )
    }
}

impl std::error::Error for PolicyError {}

/// Whether an [`EffectKind`] is a **write** (an effect, not a pure read dependency). `Read`
/// and `List` are pure dependencies of a read plan; everything else mutates / fires an effect.
#[must_use]
pub fn is_write_effect(kind: &EffectKind) -> bool {
    !matches!(kind, EffectKind::Read | EffectKind::List)
}

/// Assert that `plan` is read-only, unless `policy` grants the write. Default-deny: the first
/// write effect found is a [`PolicyError`] naming it — UNLESS a non-empty [`PolicyDef`] is
/// present (the t34 hook: today any present policy with a non-empty `allow` list opens the
/// write gate; t34 will match the specific capability). A pure read plan (no write effects)
/// always passes.
///
/// # Errors
/// [`PolicyError`] if the plan contains a write effect and `policy` does not grant it.
pub fn assert_read_only(plan: &Plan, policy: Option<&PolicyDef>) -> Result<(), PolicyError> {
    let first_write = plan
        .nodes()
        .iter()
        .find(|n| is_write_effect(&n.kind))
        .map(|n| n.kind.label().to_string());

    match first_write {
        None => Ok(()),
        Some(effect) => {
            if policy_grants_writes(policy) {
                // The t34 hook: a present, non-empty policy opens the gate. The full
                // capability-matching engine (which write maps to which `allow` scope) is t34.
                Ok(())
            } else {
                Err(PolicyError::new(effect))
            }
        }
    }
}

/// Whether `policy` grants writes for this gate. The t32 hook: a present policy with a
/// non-empty `allow` list is treated as granting (t34 refines this to per-capability matching).
/// `None` (the read-only default) never grants.
fn policy_grants_writes(policy: Option<&PolicyDef>) -> bool {
    policy.is_some_and(|p| !p.allow.is_empty())
}
