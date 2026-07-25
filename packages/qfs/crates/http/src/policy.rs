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
//! ## Reads are gated too (the enforcement half)
//! A pure read lowers to an EMPTY commit plan, so the plan gate above sees nothing and used to pass
//! EVERY read regardless of policy — "who is asking" was resolved and then ignored. [`assert_select_allowed`]
//! closes that: the read's [`ScanTarget`]s are adjudicated as `SELECT` effects through
//! [`evaluate_reads_with_context`] BEFORE the driver scan runs, under the SAME resolved actor and
//! the SAME rules the write gate uses. Fail-closed is unchanged and now reaches reads: no attached
//! policy, or a dangling policy ref, resolves to the default-deny [`Policy`], so an ungranted read
//! is a 403 refusal rather than an empty relation at 200.
//!
//! Default-deny / fail-closed: ANY effect — a write the policy does not grant, or a read path it
//! does not grant — is a [`PolicyError`]. This is the **may** layer; the t13 driver capability
//! (**can**) is a distinct, earlier gate.

use qfs_core::{EffectKind, Plan, RequestContext};
use qfs_exec::ScanTarget;
use qfs_server::{
    evaluate_reads_with_context, evaluate_with_context, policy_from_def, DecisionContext, Policy,
    PolicyDecision, PolicyDef, PolicyTable, ReadTarget,
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
        // A read denial and a write denial are different refusals and say so: an operator reading
        // "performs a write effect (SELECT)" would go looking for a write that does not exist.
        if self.effect == qfs_server::Verb::Select.label() {
            return write!(
                f,
                "endpoint query reads a path (SELECT) the bound POLICY does not grant: {}",
                self.reason
            );
        }
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
    assert_plan_allowed(plan, &resolved, actor)
}

/// Resolve an endpoint's bound `policy` REF against the live policy table — the fail-closed
/// resolution the whole serve seam shares ([`qfs_server::resolve_policy`]).
///
/// - a named policy present in the table ⇒ that policy;
/// - a **dangling** ref (named, absent) ⇒ a named default-deny policy;
/// - **no** ref at all ⇒ the default-deny [`Policy::default`].
///
/// The ref is the endpoint's own `policy:` field, NOT its name: looking a policy up by endpoint name
/// would silently apply the wrong policy (or none) to a request whose endpoint declared one.
#[must_use]
pub fn resolve_endpoint_policy(policy_ref: Option<&str>, table: &PolicyTable) -> Policy {
    qfs_server::resolve_policy(policy_ref, table)
}

/// Assert that `plan`'s write/CALL effects are permitted under an already-resolved `policy` for
/// `actor`. The [`assert_read_only`] core, split out so the request path can resolve the endpoint's
/// policy ONCE and hand the same resolved value to both the write gate and the read gate.
///
/// # Errors
/// [`PolicyError`] if [`evaluate_with_context`] denies any write effect in the plan.
pub fn assert_plan_allowed(
    plan: &Plan,
    policy: &Policy,
    actor: &DecisionContext,
) -> Result<(), PolicyError> {
    to_result(evaluate_with_context(policy, plan, actor))
}

/// Assert that every path this request is about to **read** is granted `SELECT` by `policy` for the
/// resolved `actor` — the read-path gate, run BEFORE any driver scan.
///
/// `targets` are the scan leaves [`qfs_exec::scan_targets`] derived from the bound statement, so the
/// gate adjudicates exactly the paths the executor would then ask a driver for. Each is evaluated as
/// a `SELECT` effect: `driver` feeds the `ON` glob, `path` the `AT` scope, and `actor` the
/// `FOR`/`WHERE` axes. Default-deny is preserved end to end — an empty/absent policy grants no read.
///
/// # Errors
/// [`PolicyError`] if [`evaluate_reads_with_context`] denies any read target.
pub fn assert_select_allowed(
    targets: &[ScanTarget],
    policy: &Policy,
    actor: &DecisionContext,
) -> Result<(), PolicyError> {
    let reads: Vec<ReadTarget> = targets
        .iter()
        .map(|t| ReadTarget::new(t.source.clone(), t.path.clone()))
        .collect();
    to_result(evaluate_reads_with_context(policy, &reads, actor))
}

/// Turn a [`PolicyDecision`] into the gate's `Result`, carrying the denied verb's stable label and
/// the secret-free reason.
fn to_result(decision: PolicyDecision) -> Result<(), PolicyError> {
    match decision {
        PolicyDecision::Allow => Ok(()),
        ref d @ PolicyDecision::Deny { ref verb, .. } => Err(PolicyError::new(
            verb.label(),
            d.deny_reason().unwrap_or_default(),
        )),
    }
}
