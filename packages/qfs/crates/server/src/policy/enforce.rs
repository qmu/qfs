//! The **pure** policy enforcer (blueprint §3 purity invariant / §8 least privilege).
//!
//! [`evaluate`] is a pure function over `(Policy, Plan)`: it walks the effect DAG, derives
//! `(Verb, driver, path)` from each effect node's **already-carried** `kind` + `target` (E2
//! nodes carry these — the enforcer reads them, never re-derives from driver internals),
//! evaluates the policy's rules top-down, and returns the FIRST denial (with the offending
//! node id, verb, driver, and the matching rule index) or [`PolicyDecision::Allow`] if every
//! effect is permitted.
//!
//! ## Default-deny (fail closed)
//! No matching rule ⇒ the policy's `default` (which is `Deny` for the default/empty policy).
//! A handler with no policy, or an empty policy, therefore **denies every write effect**.
//!
//! ## What is evaluated
//! [`evaluate_with_context`] gates the **write/CALL** effects (INSERT/UPSERT/UPDATE/REMOVE/CALL,
//! and `/server` config writes) a COMMIT plan carries. `Read`/`List` nodes inside such a plan are
//! pure dependencies of the write (blueprint §7) and stay skipped there.
//!
//! A **pure read never reaches that walk at all**: it lowers to an EMPTY commit plan and routes
//! through the separate read path. [`evaluate_reads_with_context`] is that path's gate — the same
//! rules, the same first-match order, the same default-deny, evaluated as [`Verb::Select`] over the
//! [`ReadTarget`]s the read is about to scan. Both faces share [`decide_effect`], so a grant can
//! never mean one thing to a write and another to a read.
//!
//! ## can ∧ may
//! This is the **may** layer only (does the *handler's policy* permit the verb). The t13
//! capability check ("can the *driver* do the verb") is a distinct, earlier gate with its own
//! error; the two are kept legibly separate — a policy denial never masquerades as a
//! capability error and vice versa.

use qfs_core::{EffectKind, Plan};

use super::context::DecisionContext;
use super::model::{Effectivity, Policy, Rule, Verb};

/// The result of evaluating a [`Policy`] against a [`Plan`] (blueprint §8). `Allow` permits the
/// whole plan; `Deny` carries the FIRST offending effect node + the matching rule index (or
/// `None` when the default-deny fired with no matching rule), all secret-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Every write effect in the plan is permitted.
    Allow,
    /// The plan is denied. Carries the offending node and the reason coordinates.
    Deny {
        /// The plan-local node id of the first denied effect — or, on the read path
        /// ([`evaluate_reads_with_context`]), the zero-based index of the denied
        /// [`ReadTarget`] in the scan list. Secret-free either way.
        node: u32,
        /// The verb of the denied effect.
        verb: Verb,
        /// The driver the denied effect targets (secret-free name only).
        driver: String,
        /// The matching rule index that denied it, or `None` if the default-deny fired (no
        /// rule matched at all).
        rule: Option<usize>,
        /// t37 OBS-2: when a broad `ALLOW ALL` *would* have matched but was held back because the
        /// verb is irreversible (REMOVE/CALL), this records the held-back rule index so the deny
        /// reason reads as "a broad ALL does not grant irreversible verbs" rather than a generic
        /// default-deny. `None` for an ordinary default-deny (no near-match). Secret-free.
        held_by_broad_all: Option<usize>,
        /// t57: when the default-deny fired because a rule matched the verb/driver but failed one
        /// of the richer axes (subject / realm-scope / `member_of` condition), this names the
        /// *failing axis* (secret-free — `"actor"`, `"scope /members/alice/**"`,
        /// `"member_of('/directories/...')"`) so a narrowed denial stays legible rather than
        /// reading as an unscoped default-deny. `None` when no near-match was found.
        detail: Option<String>,
    },
}

impl PolicyDecision {
    /// Whether the decision permits the plan.
    #[must_use]
    pub fn is_allow(&self) -> bool {
        matches!(self, PolicyDecision::Allow)
    }

    /// A secret-free, AI-legible denial reason (driver name + verb + rule index only — never
    /// payloads or credentials, blueprint §8). `None` for an `Allow`.
    #[must_use]
    pub fn deny_reason(&self) -> Option<String> {
        match self {
            PolicyDecision::Allow => None,
            PolicyDecision::Deny {
                node,
                verb,
                driver,
                rule,
                held_by_broad_all,
                detail,
            } => Some(match (rule, held_by_broad_all, detail) {
                // t37 OBS-2: a broad `ALLOW ALL` matched the driver/verb but is held back because
                // the verb is irreversible — say so explicitly, so the operator does not read it
                // as an ordinary default-deny and reach for an unrelated fix.
                (None, Some(all_idx), _) => format!(
                    "policy denies {} on driver `{}` (node #{node}): a broad `ALLOW ALL` \
                     (rule {all_idx}) does not grant the irreversible verb — add an explicit \
                     `ALLOW {}` to permit it",
                    verb.label(),
                    driver,
                    verb.label()
                ),
                (Some(idx), _, _) => format!(
                    "policy denies {} on driver `{}` (node #{node}, rule {idx})",
                    verb.label(),
                    driver
                ),
                // t57: a rule matched the verb/driver but the actor/scope/condition axis failed —
                // name the failing axis so the narrowed denial is legible.
                (None, None, Some(axis)) => format!(
                    "policy denies {} on driver `{}` (node #{node}, default-deny: a rule matched \
                     the verb/driver but the {axis} did not apply to the actor)",
                    verb.label(),
                    driver
                ),
                (None, None, None) => format!(
                    "policy denies {} on driver `{}` (node #{node}, default-deny: no rule \
                     matched)",
                    verb.label(),
                    driver
                ),
            }),
        }
    }
}

/// One **read target** the read path is about to scan: the source/driver name plus the addressed
/// VFS path the `FROM` named. Owned and secret-free — driver + path only, never a predicate value
/// or a credential.
///
/// This is the read path's analogue of a plan effect node's `(driver, target)`: a pure SELECT
/// produces an EMPTY commit plan, so there is no node to read the coordinates off. The serve seam
/// derives these from the physical plan's scan leaves and hands them to
/// [`evaluate_reads_with_context`], which evaluates each as a [`Verb::Select`] effect — with `path`
/// feeding the `AT` [`ScopeGlob`](super::model::ScopeGlob) axis exactly as a write's target does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadTarget {
    /// The source/driver the scan runs against (secret-free name only).
    pub driver: String,
    /// The full addressed VFS path the scan reads (`/mock/items`, `/members/alice/mail`).
    pub path: String,
}

impl ReadTarget {
    /// Construct a read target from owned driver + path text.
    #[must_use]
    pub fn new(driver: impl Into<String>, path: impl Into<String>) -> Self {
        ReadTarget {
            driver: driver.into(),
            path: path.into(),
        }
    }
}

/// How an effect node is classified for policy purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectClass {
    /// A pure read dependency (`Read`/`List`) — NOT policy-bearing (a read goes through the
    /// separate read path; a write's read dependency is not gated).
    Read,
    /// A policy-bearing write/CALL effect, classified as `verb`.
    Verb(Verb),
    /// An effect kind the policy layer does not recognize (a NEW `EffectKind` variant added
    /// after this layer was written). Fail-closed: the enforcer DENIES it — a new effect must
    /// not be silently permitted (blueprint §8 default-deny).
    Unknown,
}

/// Classify a plan [`EffectKind`]. `Read`/`List` are pure dependencies (skipped); the write
/// verbs map to their [`Verb`]; an unrecognized future kind is [`EffectClass::Unknown`]
/// (fail-closed denied).
#[must_use]
pub fn classify_effect(kind: &EffectKind) -> EffectClass {
    match kind {
        EffectKind::Read | EffectKind::List => EffectClass::Read,
        EffectKind::Insert => EffectClass::Verb(Verb::Insert),
        EffectKind::Upsert => EffectClass::Verb(Verb::Upsert),
        EffectKind::Update => EffectClass::Verb(Verb::Update),
        EffectKind::Remove => EffectClass::Verb(Verb::Remove),
        EffectKind::Call(_) => EffectClass::Verb(Verb::Call),
        // A `/server` self-config write maps to the verb its op implies — these are governed
        // exactly like any other effect (a handler that rewrites `/server` must be granted it).
        EffectKind::ServerConfigWrite { op, .. } => EffectClass::Verb(match op {
            qfs_core::ServerWriteOp::Insert => Verb::Insert,
            qfs_core::ServerWriteOp::Upsert => Verb::Upsert,
            qfs_core::ServerWriteOp::Update => Verb::Update,
            qfs_core::ServerWriteOp::Remove => Verb::Remove,
        }),
        // A future `EffectKind` variant: fail-closed (deny). The match is intentionally NOT a
        // bare `_` for the known set — only this one catch-all for genuinely-new variants.
        _ => EffectClass::Unknown,
    }
}

/// Back-compat helper: the policy [`Verb`] of an effect, or `None` for a read dependency.
/// (An [`EffectClass::Unknown`] also yields `None`; callers that need fail-closed handling use
/// [`classify_effect`] directly — the enforcer does.)
#[must_use]
pub fn verb_for_effect(kind: &EffectKind) -> Option<Verb> {
    match classify_effect(kind) {
        EffectClass::Verb(v) => Some(v),
        EffectClass::Read | EffectClass::Unknown => None,
    }
}

/// Evaluate `policy` against `plan` under the **anonymous** decision context (blueprint §8). Pure:
/// no I/O, no mutation. This is the back-compat entry point — equivalent to
/// [`evaluate_with_context`] with [`DecisionContext::anonymous`]. Under the anonymous context
/// only unscoped (`FOR anyone`, no condition) rules can match, so a pre-t57 policy behaves
/// exactly as before, and a t57-narrowed rule contributes nothing until a real actor is
/// resolved (fail closed).
#[must_use]
pub fn evaluate(policy: &Policy, plan: &Plan) -> PolicyDecision {
    evaluate_with_context(policy, plan, &DecisionContext::anonymous())
}

/// Whether `rule` matches the effect `(verb, driver, path)` **for the resolved actor `ctx`**
/// (t57). All five axes must hold: the verb+driver glob (with the irreversible-strictness rule),
/// the [`Subject`](super::model::Subject), the realm-scoped path, and the
/// [`Condition`](super::model::Condition). Pure — `ctx` is already resolved, so this performs no
/// I/O.
fn rule_matches_in_context(
    rule: &Rule,
    verb: Verb,
    driver: &str,
    path: &str,
    ctx: &DecisionContext,
) -> bool {
    rule.matches(verb, driver, path)
        && ctx.satisfies_subject(&rule.subject)
        && rule.scope.as_ref().is_none_or(|s| s.matches_path(path))
        && ctx.satisfies_condition(&rule.condition)
}

/// Evaluate `policy` against `plan` for a **resolved** [`DecisionContext`] (t57). Pure: no I/O,
/// no mutation — the actor's identity/roles/memberships were resolved up front (see
/// [`super::context::resolve_memberships`]) and frozen into `ctx`, so this is a total function
/// over `(policy, plan, ctx)`.
///
/// Walks the effect nodes in plan order, classifies each into a `(verb, driver, path)`,
/// evaluates the rules top-down (the FIRST matching rule decides — so an earlier `DENY` overrides
/// a later `ALLOW`; deny-by-precedence), and returns the FIRST denial or [`PolicyDecision::Allow`]
/// if every write effect is permitted. Default-deny: a write effect that no rule matches (or whose
/// matching rules all failed the actor/scope/condition axes) falls to `policy.default` — `Deny`
/// for the default/empty policy.
#[must_use]
pub fn evaluate_with_context(
    policy: &Policy,
    plan: &Plan,
    ctx: &DecisionContext,
) -> PolicyDecision {
    for node in plan.nodes() {
        let driver = node.target.driver.as_str().to_string();
        let path = node.target.path.as_str();
        // Only write/CALL effects are policy-bearing; read dependencies are skipped; an
        // unrecognized future kind is denied fail-closed (default-deny, blueprint §8).
        let verb = match classify_effect(&node.kind) {
            EffectClass::Read => continue,
            EffectClass::Verb(v) => v,
            EffectClass::Unknown => {
                return PolicyDecision::Deny {
                    node: node.id.index(),
                    // No owned verb for an unknown kind; report the closest irreversible
                    // class so the operator treats it with maximal caution.
                    verb: Verb::Call,
                    driver,
                    rule: None,
                    held_by_broad_all: None,
                    detail: None,
                };
            }
        };

        if let Some(deny) = decide_effect(policy, node.id.index(), verb, driver, path, ctx) {
            return deny;
        }
    }
    PolicyDecision::Allow
}

/// Evaluate `policy` against the **read targets** a pure read is about to scan, for a resolved
/// [`DecisionContext`] (the mission's enforcement half). Pure: no I/O, no mutation.
///
/// A pure read lowers to an EMPTY commit plan, so [`evaluate_with_context`] sees nothing to gate —
/// which is exactly why a read used to pass under ANY policy. This is the read path's gate: each
/// target is evaluated as a [`Verb::Select`] effect through the SAME [`decide_effect`] the write
/// walk uses (first matching rule decides; an earlier `DENY` beats a later `ALLOW`; no match falls
/// to `policy.default`, which is `Deny` for the default/empty policy). The target's `path` feeds the
/// `AT` scope axis, its `driver` the `ON` driver glob, and `ctx` the `FOR`/`WHERE` axes — so one
/// policy governs reads and writes with one vocabulary.
///
/// Returns the FIRST denial, or [`PolicyDecision::Allow`] when every target is granted. An EMPTY
/// target list is an `Allow`: there is nothing to read, so there is nothing to refuse.
#[must_use]
pub fn evaluate_reads_with_context(
    policy: &Policy,
    reads: &[ReadTarget],
    ctx: &DecisionContext,
) -> PolicyDecision {
    for (idx, target) in reads.iter().enumerate() {
        let node = u32::try_from(idx).unwrap_or(u32::MAX);
        if let Some(deny) = decide_effect(
            policy,
            node,
            Verb::Select,
            target.driver.clone(),
            &target.path,
            ctx,
        ) {
            return deny;
        }
    }
    PolicyDecision::Allow
}

/// Decide ONE `(verb, driver, path)` against `policy` for the resolved actor `ctx`: returns the
/// [`PolicyDecision::Deny`] when the policy refuses it, or `None` when it is permitted.
///
/// The single decision procedure BOTH faces share — the write/CALL plan walk
/// ([`evaluate_with_context`]) and the read walk ([`evaluate_reads_with_context`]). Keeping it in
/// one place is what makes per-face permission drift structurally impossible: first-matching-rule
/// order, the fail-closed `policy.default`, the broad-`ALL` irreversible hold-back, and the
/// secret-free near-miss axis are computed once and mean the same thing to every caller.
fn decide_effect(
    policy: &Policy,
    node: u32,
    verb: Verb,
    driver: String,
    path: &str,
    ctx: &DecisionContext,
) -> Option<PolicyDecision> {
    // Walk rules top-down; the first rule that matches (in this actor's context) decides this
    // effect. First-match means an earlier DENY wins over a later ALLOW.
    let mut decided: Option<(Effectivity, Option<usize>)> = None;
    for (idx, rule) in policy.rules.iter().enumerate() {
        if rule_matches_in_context(rule, verb, &driver, path, ctx) {
            decided = Some((rule.effect, Some(idx)));
            break;
        }
    }
    // No rule matched ⇒ fall to the policy default (fail-closed for the default policy).
    let (effect, rule) = decided.unwrap_or((policy.default, None));
    if effect != Effectivity::Deny {
        return None;
    }

    // t37 OBS-2: when this denial is a default-deny of an irreversible verb, detect a broad
    // `ALLOW ALL` allow rule that matched the driver/verbset but was held back by the
    // irreversible-strictness rule — so the reason can name that near-match instead of reading as
    // a generic default-deny. Pure: scans the rules already in hand.
    let held_by_broad_all = if rule.is_none() && verb.is_irreversible_class() {
        policy.rules.iter().position(|r| {
            r.effect == Effectivity::Allow
                && r.is_broad_all()
                && r.verbs.contains(verb)
                && r.driver.matches(&driver, path)
        })
    } else {
        None
    };
    // t57: when the default-deny fired but a rule DID match the verb/driver and only the
    // actor/scope/condition axis held it back, name that failing axis (secret-free) so the denial
    // is legible as a narrowed grant that did not apply, not a missing rule.
    let detail = if rule.is_none() && held_by_broad_all.is_none() {
        near_miss_axis(policy, verb, &driver, path, ctx)
    } else {
        None
    };
    Some(PolicyDecision::Deny {
        node,
        verb,
        driver,
        rule,
        held_by_broad_all,
        detail,
    })
}

/// Find the first rule that matched the verb+driver but failed one of the t57 axes, and name the
/// failing axis (secret-free). Pure: scans the rules already in hand. `None` if no near-match.
fn near_miss_axis(
    policy: &Policy,
    verb: Verb,
    driver: &str,
    path: &str,
    ctx: &DecisionContext,
) -> Option<String> {
    for rule in &policy.rules {
        if !rule.matches(verb, driver, path) {
            continue;
        }
        if !ctx.satisfies_subject(&rule.subject) {
            // blueprint §19 axis B: when the acting principal is an AGENT, name it in the failing
            // axis so a narrowed denial reads as "the agent has no matching grant" (legible,
            // secret-free) rather than an anonymous default-deny. A non-agent context keeps the
            // pre-§19 generic `"actor"` axis label (the existing user/role tests are unchanged).
            return Some(match &ctx.agent {
                Some(agent) => format!("subject (agent:{agent} has no matching grant)"),
                None => "actor".to_string(),
            });
        }
        if let Some(scope) = &rule.scope {
            if !scope.matches_path(path) {
                return Some(format!("scope {}", scope.render()));
            }
        }
        if !ctx.satisfies_condition(&rule.condition) {
            // The condition label is secret-free (a directory ref, never a credential).
            if let Some(label) = rule.condition.label() {
                return Some(label);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::context::DecisionContext;
    use crate::policy::model::{
        Condition, DriverGlob, RoleGraph, Rule, ScopeGlob, Subject, VerbSet,
    };
    use qfs_core::{DriverId, EffectNode, NodeId, ProcId, Target, VfsPath};

    fn write_node(id: u32, kind: EffectKind, driver: &str, path: &str) -> EffectNode {
        EffectNode::new(
            NodeId(id),
            kind,
            Target::new(DriverId::new(driver), VfsPath::new(path)),
        )
    }

    fn plan_of(nodes: Vec<EffectNode>) -> Plan {
        let mut p = Plan::pure();
        p.nodes = nodes;
        p
    }

    #[test]
    fn empty_policy_denies_every_effect() {
        let policy = Policy::default();
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/mail/inbox",
        )]);
        match evaluate(&policy, &plan) {
            PolicyDecision::Deny {
                node, verb, rule, ..
            } => {
                assert_eq!(node, 0);
                assert_eq!(verb, Verb::Insert);
                assert_eq!(rule, None, "default-deny carries no rule index");
            }
            PolicyDecision::Allow => panic!("empty policy must deny"),
        }
    }

    /// The INVERSION of the retired `select_only_plan_is_allowed_even_under_empty_policy`.
    ///
    /// That test pinned the inert-on-reads state: a pure read lowered to an empty commit plan, so
    /// the enforcer had nothing to gate and every read passed under every policy. The read path now
    /// has its own gate ([`evaluate_reads_with_context`]) and an empty policy — whose `default` is
    /// `Deny` — REFUSES the read. This test is the proof the inert state is gone; qfs is
    /// experimental, so the old behaviour is broken deliberately with no compatibility shim.
    #[test]
    fn select_on_the_read_path_is_denied_under_empty_policy() {
        let policy = Policy::default();
        let reads = [ReadTarget::new("mail", "/mail/inbox")];
        match evaluate_reads_with_context(&policy, &reads, &DecisionContext::anonymous()) {
            PolicyDecision::Deny {
                node, verb, driver, ..
            } => {
                assert_eq!(node, 0, "the first denied read target");
                assert_eq!(verb, Verb::Select, "a read is classified as SELECT");
                assert_eq!(driver, "mail");
            }
            PolicyDecision::Allow => panic!("an empty policy must deny the read (default-deny)"),
        }
        // An explicit grant opens it — the same rule vocabulary a write uses.
        let granted = Policy::new("reader")
            .with_rule(Rule::allow(VerbSet::one(Verb::Select), DriverGlob::any()));
        assert!(
            evaluate_reads_with_context(&granted, &reads, &DecisionContext::anonymous()).is_allow()
        );
    }

    /// The counterpart invariant: a `Read` DEPENDENCY node inside a WRITE plan is still skipped by
    /// the plan walk. Enabling read enforcement must not silently re-gate every write's read leg —
    /// the write-side decision matrix is unchanged (mission acceptance 3, "nothing widens" also
    /// means "nothing narrows by accident").
    #[test]
    fn read_dependency_of_a_write_plan_is_still_skipped_by_the_plan_walk() {
        // A policy that grants INSERT but NOT select: the write's read dependency must not deny it.
        let policy = Policy::new("writer")
            .with_rule(Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any()));
        let plan = plan_of(vec![
            write_node(0, EffectKind::Read, "mail", "/mail/inbox"),
            write_node(1, EffectKind::Insert, "log", "/log"),
        ]);
        assert!(
            evaluate(&policy, &plan).is_allow(),
            "a write's Read dependency is not a policy-bearing effect (blueprint §7)"
        );
        assert_eq!(classify_effect(&EffectKind::Read), EffectClass::Read);
        assert_eq!(classify_effect(&EffectKind::List), EffectClass::Read);
    }

    #[test]
    fn allow_insert_permits_insert_denies_remove() {
        let policy = Policy::new("api")
            .with_rule(Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any()));
        let allowed = plan_of(vec![write_node(0, EffectKind::Insert, "log", "/log")]);
        assert!(evaluate(&policy, &allowed).is_allow());

        let denied = plan_of(vec![write_node(1, EffectKind::Remove, "log", "/log")]);
        match evaluate(&policy, &denied) {
            PolicyDecision::Deny { verb, .. } => assert_eq!(verb, Verb::Remove),
            PolicyDecision::Allow => panic!("REMOVE not granted ⇒ deny"),
        }
    }

    #[test]
    fn call_is_denied_without_explicit_allow_call() {
        let policy = Policy::new("api")
            .with_rule(Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any()));
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Call(ProcId::new("mail.send")),
            "mail",
            "/mail/outbox",
        )]);
        match evaluate(&policy, &plan) {
            PolicyDecision::Deny { verb, driver, .. } => {
                assert_eq!(verb, Verb::Call);
                assert_eq!(driver, "mail");
            }
            PolicyDecision::Allow => panic!("CALL not granted ⇒ deny"),
        }
    }

    #[test]
    fn allow_all_token_does_not_grant_irreversible() {
        // A broad `ALLOW ALL` grants reversible writes but NOT REMOVE/CALL.
        let policy = Policy::new("broad")
            .with_rule(Rule::allow(VerbSet::all(), DriverGlob::any()).as_all_token());

        let insert = plan_of(vec![write_node(0, EffectKind::Insert, "log", "/log")]);
        assert!(evaluate(&policy, &insert).is_allow(), "ALL grants INSERT");

        let remove = plan_of(vec![write_node(0, EffectKind::Remove, "log", "/log")]);
        let decision = evaluate(&policy, &remove);
        assert!(!decision.is_allow(), "ALL must NOT grant REMOVE");
        // t37 OBS-2: the deny reason names the held-back broad ALL, not a generic default-deny.
        match &decision {
            PolicyDecision::Deny {
                held_by_broad_all, ..
            } => assert!(
                held_by_broad_all.is_some(),
                "an irreversible verb held back by a broad ALL must record the near-match rule"
            ),
            PolicyDecision::Allow => panic!("must deny"),
        }
        let reason = decision.deny_reason().unwrap();
        assert!(
            reason.contains("broad `ALLOW ALL`") && reason.contains("ALLOW REMOVE"),
            "OBS-2 reason should explain the broad-ALL hold-back: {reason}"
        );

        let call = plan_of(vec![write_node(
            0,
            EffectKind::Call(ProcId::new("mail.send")),
            "mail",
            "/mail",
        )]);
        assert!(
            !evaluate(&policy, &call).is_allow(),
            "ALL must NOT grant CALL"
        );
    }

    #[test]
    fn explicit_verb_list_grants_irreversible() {
        // An explicit `ALLOW REMOVE,CALL` DOES grant them (not a broad ALL token).
        let policy = Policy::new("cleanup").with_rule(Rule::allow(
            VerbSet::from_verbs(&[Verb::Remove, Verb::Call]),
            DriverGlob::any(),
        ));
        let remove = plan_of(vec![write_node(0, EffectKind::Remove, "log", "/log")]);
        assert!(evaluate(&policy, &remove).is_allow());
    }

    #[test]
    fn driver_scoped_rule_denies_other_driver() {
        let policy = Policy::new("mailonly").with_rule(Rule::allow(
            VerbSet::one(Verb::Insert),
            DriverGlob::new("mail"),
        ));
        let mail = plan_of(vec![write_node(0, EffectKind::Insert, "mail", "/mail/x")]);
        assert!(evaluate(&policy, &mail).is_allow());
        let other = plan_of(vec![write_node(0, EffectKind::Insert, "s3", "/s3/x")]);
        assert!(!evaluate(&policy, &other).is_allow());
    }

    #[test]
    fn sys_policies_write_is_default_denied_then_granted() {
        // t53: a `/sys/*` write is high-privilege and routes through the SAME default-deny policy
        // engine as any other driver (the path is the authorization subject). An empty/default
        // policy denies `INSERT INTO /sys/policies`; an explicit `ALLOW INSERT on driver sys`
        // grants it. This is what "policy-gated" means for the admin surface — no special case.
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "sys",
            "/sys/policies",
        )]);
        // Default-deny: a super-admin grant is NOT implicit.
        match evaluate(&Policy::default(), &plan) {
            PolicyDecision::Deny { verb, driver, .. } => {
                assert_eq!(verb, Verb::Insert);
                assert_eq!(driver, "sys");
            }
            PolicyDecision::Allow => panic!("/sys writes must be default-denied"),
        }
        // An explicit grant scoped to the `sys` driver permits the policy-grant insert.
        let granted = Policy::new("admin").with_rule(Rule::allow(
            VerbSet::one(Verb::Insert),
            DriverGlob::new("sys"),
        ));
        assert!(evaluate(&granted, &plan).is_allow());
    }

    #[test]
    fn first_denial_is_returned() {
        let policy = Policy::new("api")
            .with_rule(Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any()));
        let plan = plan_of(vec![
            write_node(0, EffectKind::Insert, "log", "/log"),
            write_node(1, EffectKind::Remove, "log", "/log"),
            write_node(2, EffectKind::Remove, "s3", "/s3"),
        ]);
        match evaluate(&policy, &plan) {
            PolicyDecision::Deny { node, .. } => assert_eq!(node, 1, "first denial wins"),
            PolicyDecision::Allow => panic!(),
        }
    }

    // ---- t57: actor / role-scoped rules, scoped-path conditions, member_of ----------------

    /// An `admin`-role rule grants the verb only to an actor whose resolved (inheritance-expanded)
    /// role set includes `admin`; everyone else falls to the fail-closed default-deny.
    #[test]
    fn role_scoped_rule_grants_only_the_matching_actor() {
        let policy = Policy::new("ops").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::new("mail"))
                .for_subject(Subject::Role("admin".into())),
        );
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/mail/outbox",
        )]);

        // An admin (directly, or via inheritance owner⊃admin) ⇒ allow.
        let graph = RoleGraph::new().inherits("owner", "admin");
        let admin = DecisionContext::for_user("a").with_roles(["admin".to_string()], &graph);
        assert!(evaluate_with_context(&policy, &plan, &admin).is_allow());
        let owner = DecisionContext::for_user("o").with_roles(["owner".to_string()], &graph);
        assert!(
            evaluate_with_context(&policy, &plan, &owner).is_allow(),
            "owner inherits admin (additive inheritance)"
        );

        // A plain member ⇒ default-deny, and the deny names the failing *actor* axis (legible).
        let member = DecisionContext::for_user("m").with_roles(["member".to_string()], &graph);
        let decision = evaluate_with_context(&policy, &plan, &member);
        match &decision {
            PolicyDecision::Deny { rule, detail, .. } => {
                assert_eq!(*rule, None, "narrowed rule did not match ⇒ default-deny");
                assert_eq!(detail.as_deref(), Some("actor"), "names the failing axis");
            }
            PolicyDecision::Allow => panic!("a non-admin must be denied"),
        }
        // The anonymous default path also denies (default-deny still holds for an unmatched actor).
        assert!(!evaluate(&policy, &plan).is_allow());
    }

    /// A realm-scoped rule grants only within its realm sub-tree; a node in another realm (or a
    /// different principal) falls to default-deny.
    #[test]
    fn scoped_path_rule_matches_within_its_realm_only() {
        let policy = Policy::new("alice-mail").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any())
                .scoped(ScopeGlob::parse("/members/alice/**").unwrap()),
        );
        let ctx = DecisionContext::anonymous(); // scope is actor-independent here

        let in_scope = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/members/alice/mail/outbox",
        )]);
        assert!(evaluate_with_context(&policy, &in_scope, &ctx).is_allow());

        // Same realm, different principal ⇒ deny (the scope names the failing axis).
        let other_principal = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/members/bob/mail/outbox",
        )]);
        match evaluate_with_context(&policy, &other_principal, &ctx) {
            PolicyDecision::Deny { detail, .. } => {
                assert_eq!(detail.as_deref(), Some("scope /members/alice/**"));
            }
            PolicyDecision::Allow => panic!("another principal must be denied"),
        }

        // Different realm ⇒ deny (the realm gate, decision P).
        let other_realm = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/projects/alice/mail/outbox",
        )]);
        assert!(!evaluate_with_context(&policy, &other_realm, &ctx).is_allow());
    }

    /// ADR 0009 §6 "data-only" over `/sql` (blueprint §8): a path-scoped rule grants DML on a
    /// table (`INSERT` at `/sql/<conn>/<table>`) while denying DDL on the catalog (`INSERT` at
    /// `/sql/<conn>` — create table) under the SAME policy, though the verb is identical. This is
    /// the deny/allow matrix the ticket makes enforceable, proven end-to-end through the policy
    /// engine; the denial is a policy decision (`scope …`), never a missing `--commit-irreversible`.
    #[test]
    fn data_only_sql_policy_admits_dml_denies_ddl() {
        let policy = Policy::new("shop-data-only").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::new("sql"))
                .scoped(ScopeGlob::parse("/sql/*/*").unwrap()),
        );
        let ctx = DecisionContext::anonymous();

        // DML: INSERT a row into a table → allowed.
        let dml = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "sql",
            "/sql/shop/items",
        )]);
        assert!(evaluate_with_context(&policy, &dml, &ctx).is_allow());

        // DDL: INSERT into the catalog (create table) → denied, same verb, shorter path.
        let ddl = plan_of(vec![write_node(0, EffectKind::Insert, "sql", "/sql/shop")]);
        match evaluate_with_context(&policy, &ddl, &ctx) {
            PolicyDecision::Deny { detail, .. } => {
                assert_eq!(detail.as_deref(), Some("scope /sql/*/*"));
            }
            PolicyDecision::Allow => {
                panic!("DDL on the catalog must be denied by a data-only policy")
            }
        }
    }

    /// A `member_of(...)` conditional grant applies only when the directory membership was
    /// pre-resolved into the context; otherwise default-deny — and the deny reason is secret-free
    /// (it names the directory ref + verb + driver, never a credential).
    #[test]
    fn member_of_condition_gates_the_grant_and_decision_is_secret_free() {
        let dir = "/directories/google/groups/eng";
        let policy = Policy::new("eng-only").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::new("mail"))
                .when(Condition::MemberOf(dir.into())),
        );
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/mail/outbox",
        )]);

        // Member ⇒ allow (membership pre-resolved into the context).
        let member = DecisionContext::for_user("u").with_membership(dir);
        assert!(evaluate_with_context(&policy, &plan, &member).is_allow());

        // Non-member ⇒ default-deny; the reason names the directory ref but no secret/payload.
        let outsider = DecisionContext::for_user("u");
        let decision = evaluate_with_context(&policy, &plan, &outsider);
        assert!(!decision.is_allow());
        let reason = decision.deny_reason().unwrap();
        assert!(
            reason.contains("member_of"),
            "names the failing condition: {reason}"
        );
        assert!(reason.contains("mail") && reason.contains("INSERT"));
        // Secret-free: only driver/verb/condition-ref appear — assert no obvious secret markers.
        assert!(!reason.to_lowercase().contains("token"));
        assert!(!reason.to_lowercase().contains("secret"));
        assert!(!reason.to_lowercase().contains("password"));
    }

    /// Deny-precedence / first-match: an earlier `DENY` overrides a later `ALLOW` for the same
    /// actor/effect (the enforcer takes the FIRST matching rule top-down).
    #[test]
    fn earlier_deny_overrides_later_allow_first_match() {
        let policy = Policy::new("precedence")
            .with_rule(
                Rule::deny(VerbSet::one(Verb::Insert), DriverGlob::new("mail"))
                    .for_subject(Subject::Role("intern".into())),
            )
            .with_rule(Rule::allow(
                VerbSet::one(Verb::Insert),
                DriverGlob::new("mail"),
            ));
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/mail/outbox",
        )]);

        // An intern hits the earlier DENY first ⇒ deny (rule index 0).
        let graph = RoleGraph::new();
        let intern = DecisionContext::for_user("i").with_roles(["intern".to_string()], &graph);
        match evaluate_with_context(&policy, &plan, &intern) {
            PolicyDecision::Deny { rule, .. } => assert_eq!(rule, Some(0), "earlier DENY wins"),
            PolicyDecision::Allow => panic!("intern must be denied by the earlier rule"),
        }
        // A non-intern skips the DENY (subject mismatch) and hits the later ALLOW ⇒ allow.
        let other = DecisionContext::for_user("o").with_roles(["staff".to_string()], &graph);
        assert!(evaluate_with_context(&policy, &plan, &other).is_allow());
    }

    // ---- blueprint §19: the agent as a first-class policy subject --------------------------

    /// The mission's literal sentence (blueprint §19 axis B): a path the OPERATOR context reaches is
    /// DENIED to the AGENT context (default-deny), with a legible `deny_reason` naming the agent
    /// subject — and the converse, an agent-scoped grant reaches the agent, never the operator.
    #[test]
    fn agent_subject_is_first_class_and_default_deny_holds_across_identities() {
        // A grant to the operator only.
        let op_only = Policy::new("op").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::new("mail"))
                .for_subject(Subject::User("op".into())),
        );
        let plan = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/mail/outbox",
        )]);

        // The operator reaches it; the agent is default-denied on the SAME path.
        assert!(
            evaluate_with_context(&op_only, &plan, &DecisionContext::for_user("op")).is_allow()
        );
        let denied = evaluate_with_context(&op_only, &plan, &DecisionContext::for_agent("triage"));
        match &denied {
            PolicyDecision::Deny { rule, detail, .. } => {
                assert_eq!(*rule, None, "no rule matches the agent ⇒ default-deny");
                let d = detail.as_deref().unwrap_or_default();
                assert!(
                    d.contains("agent:triage"),
                    "the failing axis names the agent subject: {d}"
                );
            }
            PolicyDecision::Allow => {
                panic!("the agent must be default-denied on the operator's path")
            }
        }
        // The full deny_reason is legible + secret-free (names the agent, verb, driver; no token).
        let reason = denied.deny_reason().unwrap();
        assert!(reason.contains("agent:triage"));
        assert!(reason.contains("INSERT") && reason.contains("mail"));
        assert!(!reason.to_lowercase().contains("token"));
        assert!(!reason.to_lowercase().contains("secret"));

        // Converse: an agent-scoped grant reaches the agent, never the operator or another agent.
        let agent_only = Policy::new("ag").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::new("mail"))
                .for_subject(Subject::Agent("triage".into())),
        );
        assert!(
            evaluate_with_context(&agent_only, &plan, &DecisionContext::for_agent("triage"))
                .is_allow()
        );
        assert!(
            !evaluate_with_context(&agent_only, &plan, &DecisionContext::for_user("op")).is_allow(),
            "an operator never inherits the agent's grant"
        );
        assert!(
            !evaluate_with_context(&agent_only, &plan, &DecisionContext::for_agent("other"))
                .is_allow(),
            "a different agent is fail-closed"
        );
    }

    /// blueprint §19 axis B: `ALLOW … AT <glob> FOR agent` grants narrow path-scoped reach — the
    /// agent reaches only within its `ScopeGlob`, denied outside it (ScopeGlob/PathScope unchanged).
    #[test]
    fn agent_grant_is_path_scoped_by_at() {
        let policy = Policy::new("scoped").with_rule(
            Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any())
                .for_subject(Subject::Agent("triage".into()))
                .scoped(ScopeGlob::parse("/me/mail/**").unwrap()),
        );
        let agent = DecisionContext::for_agent("triage");
        let in_scope = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/me/mail/outbox",
        )]);
        assert!(evaluate_with_context(&policy, &in_scope, &agent).is_allow());
        let out_of_scope = plan_of(vec![write_node(
            0,
            EffectKind::Insert,
            "mail",
            "/me/other/x",
        )]);
        assert!(
            !evaluate_with_context(&policy, &out_of_scope, &agent).is_allow(),
            "the agent is denied outside its AT scope"
        );
    }

    /// A pre-t57 (unscoped) policy behaves identically under the anonymous context — the back-compat
    /// guarantee that keeps the existing 1605 tests green.
    #[test]
    fn unscoped_policy_is_unchanged_under_anonymous_context() {
        let policy = Policy::new("api")
            .with_rule(Rule::allow(VerbSet::one(Verb::Insert), DriverGlob::any()));
        let plan = plan_of(vec![write_node(0, EffectKind::Insert, "log", "/log")]);
        // Both the back-compat `evaluate` and the explicit anonymous context agree.
        assert!(evaluate(&policy, &plan).is_allow());
        assert!(evaluate_with_context(&policy, &plan, &DecisionContext::anonymous()).is_allow());
    }

    // ---- every grant axis bites on READS, both directions -----------------------------------
    //
    // The t57 axes were proven for writes. These are the read-side matrices: per axis, ONE proof
    // that the matching actor's read succeeds and ONE that the non-matching actor's is denied.
    // They reuse the same `Rule`/`DecisionContext` builders the write-side tests use — the point
    // is precisely that reads ride the SAME rules, not a parallel read-side vocabulary.

    /// The one-target read every axis test adjudicates.
    fn read_of(driver: &str, path: &str) -> Vec<ReadTarget> {
        vec![ReadTarget::new(driver, path)]
    }

    /// FOR `user:` — a read granted to Alice admits Alice and denies Bob (and anonymous).
    #[test]
    fn for_user_axis_bites_on_reads_both_directions() {
        let policy = Policy::new("alice-reads").with_rule(
            Rule::allow(VerbSet::one(Verb::Select), DriverGlob::new("mail"))
                .for_subject(Subject::User("alice".into())),
        );
        let reads = read_of("mail", "/mail/inbox");

        assert!(
            evaluate_reads_with_context(&policy, &reads, &DecisionContext::for_user("alice"))
                .is_allow(),
            "the granted user reads"
        );

        let denied =
            evaluate_reads_with_context(&policy, &reads, &DecisionContext::for_user("bob"));
        match &denied {
            PolicyDecision::Deny { rule, detail, .. } => {
                assert_eq!(
                    *rule, None,
                    "the narrowed rule did not match ⇒ default-deny"
                );
                assert_eq!(detail.as_deref(), Some("actor"), "names the failing axis");
            }
            PolicyDecision::Allow => panic!("a rule for Alice must not grant Bob"),
        }
        assert!(
            !evaluate_reads_with_context(&policy, &reads, &DecisionContext::anonymous()).is_allow(),
            "an anonymous read of a FOR-narrowed grant is denied"
        );
    }

    /// FOR `role:` — including the INHERITED case: `owner` holds what `member` was granted, while a
    /// role outside the grant is denied.
    #[test]
    fn for_role_axis_bites_on_reads_including_inheritance() {
        let policy = Policy::new("members-read").with_rule(
            Rule::allow(VerbSet::one(Verb::Select), DriverGlob::any())
                .for_subject(Subject::Role("member".into())),
        );
        let reads = read_of("mail", "/mail/inbox");
        let graph = RoleGraph::new().inherits("owner", "member");

        let member = DecisionContext::for_user("m").with_roles(["member".to_string()], &graph);
        assert!(evaluate_reads_with_context(&policy, &reads, &member).is_allow());

        let owner = DecisionContext::for_user("o").with_roles(["owner".to_string()], &graph);
        assert!(
            evaluate_reads_with_context(&policy, &reads, &owner).is_allow(),
            "owner inherits member's read grant (additive inheritance)"
        );

        let guest = DecisionContext::for_user("g").with_roles(["guest".to_string()], &graph);
        assert!(
            !evaluate_reads_with_context(&policy, &reads, &guest).is_allow(),
            "a role outside the grant reads nothing"
        );
    }

    /// FOR `group:` — a group-scoped read grant admits a member of that group and no one else.
    #[test]
    fn for_group_axis_bites_on_reads_both_directions() {
        let policy = Policy::new("eng-reads").with_rule(
            Rule::allow(VerbSet::one(Verb::Select), DriverGlob::any())
                .for_subject(Subject::Group("eng".into())),
        );
        let reads = read_of("mail", "/mail/inbox");

        let in_group = DecisionContext::for_user("u").with_groups(["eng".to_string()]);
        assert!(evaluate_reads_with_context(&policy, &reads, &in_group).is_allow());

        let out_group = DecisionContext::for_user("u").with_groups(["sales".to_string()]);
        assert!(
            !evaluate_reads_with_context(&policy, &reads, &out_group).is_allow(),
            "a different group is fail-closed"
        );
    }

    /// AT — the `ScopeGlob` path scope decides on the READ's scanned path: Alice's sub-tree is
    /// admitted, a read one principal over is denied, and the denial names the scope.
    #[test]
    fn at_scope_axis_bites_on_reads_both_directions() {
        let policy = Policy::new("alice-subtree").with_rule(
            Rule::allow(VerbSet::one(Verb::Select), DriverGlob::any())
                .scoped(ScopeGlob::parse("/members/alice/**").unwrap()),
        );
        let ctx = DecisionContext::for_user("alice");

        let in_scope = read_of("mail", "/members/alice/mail/inbox");
        assert!(evaluate_reads_with_context(&policy, &in_scope, &ctx).is_allow());

        let out_of_scope = read_of("mail", "/members/bob/mail/inbox");
        match evaluate_reads_with_context(&policy, &out_of_scope, &ctx) {
            PolicyDecision::Deny { verb, detail, .. } => {
                assert_eq!(verb, Verb::Select);
                assert_eq!(detail.as_deref(), Some("scope /members/alice/**"));
            }
            PolicyDecision::Allow => panic!("a read outside the AT scope must be denied"),
        }
    }

    /// WHERE — the `member_of('/directories/...')` conditional grant, resolved through the EXISTING
    /// membership-resolver seam (never from inside the pure enforcer), decides a read the same way
    /// it decides a write.
    #[test]
    fn where_member_of_axis_bites_on_reads_both_directions() {
        use crate::policy::context::{resolve_memberships, MembershipResolver};

        /// A test resolver: `u1` is in the eng directory, nobody else is.
        struct OnlyU1;
        impl MembershipResolver for OnlyU1 {
            fn is_member(&self, actor: Option<&str>, directory: &str) -> bool {
                actor == Some("u1") && directory == "/directories/google/groups/eng"
            }
        }

        let dir = "/directories/google/groups/eng";
        let policy = Policy::new("eng-only-read").with_rule(
            Rule::allow(VerbSet::one(Verb::Select), DriverGlob::any())
                .when(Condition::MemberOf(dir.into())),
        );
        let reads = read_of("mail", "/mail/inbox");

        // The membership is resolved UP FRONT into the context; the enforcer only does set lookup.
        let member = resolve_memberships(DecisionContext::for_user("u1"), &policy, &OnlyU1);
        assert!(evaluate_reads_with_context(&policy, &reads, &member).is_allow());

        let outsider = resolve_memberships(DecisionContext::for_user("u2"), &policy, &OnlyU1);
        let denied = evaluate_reads_with_context(&policy, &reads, &outsider);
        assert!(!denied.is_allow(), "a non-member reads nothing");
        let reason = denied.deny_reason().unwrap();
        assert!(
            reason.contains("member_of"),
            "the denial names the failing condition: {reason}"
        );
        assert!(reason.contains("SELECT"), "and the verb: {reason}");
        // Secret-free: the reason carries the directory ref, never credential material.
        assert!(!reason.to_lowercase().contains("secret"));
        assert!(!reason.to_lowercase().contains("password"));
    }

    /// The irreversible-strictness invariant is untouched by the read work: a bare `ALLOW ALL`
    /// grants the reversible verbs — SELECT included, so it DOES open a read — but still never
    /// grants REMOVE or CALL. Enabling read enforcement must not have loosened `Rule::matches`.
    #[test]
    fn broad_allow_all_grants_reads_but_still_never_grants_irreversible() {
        let policy = Policy::new("broad")
            .with_rule(Rule::allow(VerbSet::all(), DriverGlob::any()).as_all_token());
        let ctx = DecisionContext::anonymous();

        assert!(
            evaluate_reads_with_context(&policy, &read_of("mail", "/mail/inbox"), &ctx).is_allow(),
            "SELECT is reversible: a broad ALL grants it"
        );
        let remove = plan_of(vec![write_node(
            0,
            EffectKind::Remove,
            "mail",
            "/mail/inbox",
        )]);
        assert!(
            !evaluate_with_context(&policy, &remove, &ctx).is_allow(),
            "a broad ALLOW ALL must still NOT grant REMOVE"
        );
        let call = plan_of(vec![write_node(
            0,
            EffectKind::Call(ProcId::new("mail.send")),
            "mail",
            "/mail/outbox",
        )]);
        assert!(
            !evaluate_with_context(&policy, &call, &ctx).is_allow(),
            "a broad ALLOW ALL must still NOT grant CALL"
        );
    }

    /// Deny-precedence holds on reads too: an earlier `DENY SELECT` for one role beats a later
    /// blanket `ALLOW SELECT`, and the non-denied actor still reads through the later rule.
    #[test]
    fn first_match_deny_precedence_holds_on_reads() {
        let policy = Policy::new("read-precedence")
            .with_rule(
                Rule::deny(VerbSet::one(Verb::Select), DriverGlob::any())
                    .for_subject(Subject::Role("intern".into())),
            )
            .with_rule(Rule::allow(VerbSet::one(Verb::Select), DriverGlob::any()));
        let reads = read_of("mail", "/mail/inbox");
        let graph = RoleGraph::new();

        let intern = DecisionContext::for_user("i").with_roles(["intern".to_string()], &graph);
        match evaluate_reads_with_context(&policy, &reads, &intern) {
            PolicyDecision::Deny { rule, .. } => assert_eq!(rule, Some(0), "the earlier DENY wins"),
            PolicyDecision::Allow => panic!("the intern's read must be denied"),
        }
        let staff = DecisionContext::for_user("s").with_roles(["staff".to_string()], &graph);
        assert!(evaluate_reads_with_context(&policy, &reads, &staff).is_allow());
    }

    /// A read with SEVERAL scan targets (a federated join) is granted only when EVERY target is:
    /// one ungranted leaf denies the whole read, and the denial names that leaf.
    #[test]
    fn every_scanned_target_must_be_granted_not_just_the_first() {
        let policy = Policy::new("mail-only").with_rule(Rule::allow(
            VerbSet::one(Verb::Select),
            DriverGlob::new("mail"),
        ));
        let federated = vec![
            ReadTarget::new("mail", "/mail/inbox"),
            ReadTarget::new("sql", "/sql/shop/orders"),
        ];
        match evaluate_reads_with_context(&policy, &federated, &DecisionContext::anonymous()) {
            PolicyDecision::Deny {
                node, verb, driver, ..
            } => {
                assert_eq!(node, 1, "the second (ungranted) leaf is the denial");
                assert_eq!(verb, Verb::Select);
                assert_eq!(driver, "sql");
            }
            PolicyDecision::Allow => {
                panic!("a federated read is only as granted as its weakest leg")
            }
        }
    }
}
