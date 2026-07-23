//! The **policy gate** (t35, replacing the t32 read-only-default; blueprint §3 purity / §8
//! least-privilege).
//!
//! An endpoint serves the *query face* of qfs (pure reads route through the read path and
//! produce an empty commit plan), but a write-lowering endpoint COMMITs a write plan. That
//! plan is now enforced against the endpoint's bound [`qfs_server::Policy`] via the REAL pure
//! enforcer [`qfs_server::evaluate`] — the same `evaluate` the cron/watchtower committers use.
//! Enforced at BOTH:
//!   * **route compile** (registration): a write-lowering endpoint whose policy denies it never
//!     becomes a live route (the plan-assertion acceptance);
//!   * **request time** (defence in depth): even a hot-swapped route re-asserts before eval.
//!
//! Default-deny / fail-closed: a pure read (no write effects) passes; ANY write effect with no
//! granting policy (or a policy that denies the verb/driver) is a [`PolicyError`]. This is the
//! **may** layer; the t13 driver capability (**can**) is a distinct, earlier gate.

use qfs_core::{EffectKind, Plan, RequestContext};
use qfs_server::{
    evaluate_with_context, policy_from_def, DecisionContext, Policy, PolicyDecision, PolicyDef,
};

/// A policy denial: the endpoint's query lowers to a write effect its bound policy does not
/// grant. Maps to HTTP 403. Secret-free — names only the effect/verb + driver, never any data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyError {
    /// The stable label of the denied effect's verb (e.g. `INSERT`, `REMOVE`, `CALL`).
    pub effect: String,
    /// The secret-free denial reason (verb + driver + rule index), for the structured error.
    pub reason: String,
}

impl PolicyError {
    /// Construct a denial naming the offending write effect + the secret-free reason.
    #[must_use]
    pub fn new(effect: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            effect: effect.into(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "endpoint query performs a write effect ({}) the bound POLICY does not grant: {}",
            self.effect, self.reason
        )
    }
}

impl std::error::Error for PolicyError {}

/// Whether an [`EffectKind`] is a **write** (an effect, not a pure read dependency). `Read`
/// and `List` are pure dependencies of a read plan; everything else mutates / fires an effect.
/// (Used by the security tests to assert a bound malicious query lowers to no write effect.)
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_write_effect(kind: &EffectKind) -> bool {
    !matches!(kind, EffectKind::Read | EffectKind::List)
}

/// Map the request's [`RequestContext`] (the M2 principal seam) onto the policy layer's
/// [`DecisionContext`] — the actor axis the enforcer reads. A signed-in user becomes
/// [`DecisionContext::for_user`]; the anonymous (not-signed-in) request becomes the empty
/// [`DecisionContext::anonymous`], under which only unscoped rules match (fail closed). Roles /
/// groups / memberships resolve later (t57/t58); this supplies the *user* axis the gate needs to
/// stop always evaluating anonymous.
#[must_use]
pub fn decision_for(req_ctx: &RequestContext) -> DecisionContext {
    match req_ctx.user() {
        Some(id) => DecisionContext::for_user(id),
        None => DecisionContext::anonymous(),
    }
}

/// Assert that `plan` is permitted under the endpoint's bound policy (t35), **for the request's
/// resolved actor** `actor`. Resolves the bound [`PolicyDef`] into a [`Policy`] (or the
/// fail-closed default-deny when none is attached) and runs the pure [`evaluate_with_context`]
/// against `actor` — so a t57-narrowed (`FOR <subject>`) rule contributes when a principal is
/// present, instead of always evaluating the anonymous context. A pure read plan (no write
/// effects) always passes; ANY write effect the policy does not grant is a [`PolicyError`].
/// Under [`DecisionContext::anonymous`] only unscoped rules match, so fail-closed is preserved.
///
/// # Errors
/// [`PolicyError`] if `evaluate_with_context` denies any write effect in the plan.
pub fn assert_read_only(
    plan: &Plan,
    policy: Option<&PolicyDef>,
    actor: &DecisionContext,
) -> Result<(), PolicyError> {
    let resolved: Policy = match policy {
        Some(def) => policy_from_def(def),
        // No attached policy ⇒ default-deny (fail closed). A pure read plan still passes
        // because the enforcer sees no write effects.
        None => Policy::default(),
    };
    match evaluate_with_context(&resolved, plan, actor) {
        PolicyDecision::Allow => Ok(()),
        ref d @ PolicyDecision::Deny { ref verb, .. } => Err(PolicyError::new(
            verb.label(),
            d.deny_reason().unwrap_or_default(),
        )),
    }
}
