---
type: Mission
title: What a principal can see and do is granted by policy
slug: what-a-principal-can-see-and-do-is-granted-by-policy
status: achieved
created_at: 2026-07-24T01:10:45+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: access-derives-from-the-resolved-principal
drive_authorized: true
predicted_hours:
actual_hours: 0.89
tickets: []
stories: []
concerns: []
gate_type:
gate_target:
gate_assert:
---

# What a principal can see and do is granted by policy

## Goal

The predecessor mission (`a-request-resolves-to-a-principal-the-query-path-can-read`, achieved
2026-07-24, shipped in v0.0.88) delivered **"who am I"**: every request resolves to a named
principal or the first-class not-signed-in answer, on the path a query takes. But **the policy
gate is still inert on data reads**: the enforcer classifies only write/CALL effects
(`crates/server/src/policy/enforce.rs` skips `Read`/`List`; a pure read lowers to an empty commit
plan and `evaluate` returns Allow regardless of policy — proven by
`select_only_plan_is_allowed_even_under_empty_policy`). The read executor threads the principal to
`ReadDriver::scan` but **never consults a Policy**. Knowing who is asking currently grants
everything readable to everyone.

This mission is the enforcement half of the strategy: **what a caller can see and do derives from
the resolved principal through PBAC grants** — `(subject, verb, path-pattern)` rules with the
already-built axes (`Subject` FOR, `ScopeGlob` AT, `Condition` WHERE, `RoleGraph` inheritance,
`SELECT` verb, `DecisionContext`) finally evaluated on the read path. One policy then governs
every face reached through the serve seam, making per-face permission drift structurally
impossible.

**Scope ruling (developer, 2026-07-24): enforcement only.** The parked product decision — "what
may I administer", the super-admin vs project-admin split (`crates/qfs/src/sys.rs:23-27`, roadmap
§3.4) — stays parked. Ticket `20260717141600-declare-sessions-and-roles-as-principals` stays
blocked on it in the icebox. Nothing in this mission may close that decision as a side effect.

## Scope

**Done when** every acceptance item below is ticked: a SELECT over the serve path is evaluated
against the endpoint's policy with the real resolved actor, every t57 grant axis bites on reads
in both directions, fail-closed is proven for the read path (anonymous sees only Anyone-granted;
no-policy and dangling-policy deny), and a denied read is a structured, secret-free refusal.

**Out of scope — do not do these in passing:**

- **"What may I administer" / the admin split** — the parked product decision, untouched.
- **The local CLI path** — the loopback/local-CLI super-admin wiring (`sys.rs`) and the
  `SafetyMode` security floor stay as they are; this mission enforces the **serve** seam
  (HTTP/MCP faces through `dispatch_inner`), not the operator's own terminal.
- **Refusing unauthenticated requests outright** (t50/t51) — an anonymous request still gets the
  Anyone-granted view, not a 401; the signed-out state remains a first-class answer.
- **Multi-tenant federation, OAuth AS work (t48), agent principals** — other missions' territory.

## Experience

1. **A read is gated like a write.** A request whose endpoint carries a policy sees `SELECT`
   evaluated by `evaluate_with_context` with the resolved `DecisionContext` — a rule
   `ALLOW SELECT ON /members/** FOR role:member` admits a signed-in member's read and contributes
   nothing to an anonymous one. The gate consumes the principal the predecessor mission threaded.
2. **Every grant axis bites on reads, both directions.** FOR (user/role/group via RoleGraph
   inheritance), AT (ScopeGlob path scope), and WHERE (member_of) each admit the matching actor
   and deny the non-matching one, proven by hermetic tests in both directions per axis.
3. **Fail-closed is preserved and provable.** No policy ⇒ default-deny; a dangling policy ref ⇒
   default-deny; anonymous ⇒ only `Subject::Anyone` + `Condition::Always` rules match. A test
   fails if threading enforcement ever widens a default.
4. **A denial is a structured answer, not an empty relation.** A policy-denied read returns a
   secret-free structured refusal (the `deny_reason` shape) at non-zero/denied status — never
   `rows: []` at exit 0. (The sibling mission `a-where-predicate-is-honored-or-refused-never-dropped`
   owns the same honesty rule for predicates; this mission owns it for authorization.)

## Acceptance

- [x] The serve read path evaluates SELECT against the endpoint's policy with the resolved actor; the inert-on-reads behavior (empty-commit-plan ⇒ Allow) is gone and the read-allowed test flips accordingly (#20260724013000-enforce-policy-on-the-serve-read-path.md)
- [x] FOR / AT / WHERE / RoleGraph each admit and deny on reads in both directions, hermetically (#20260724013100-every-grant-axis-bites-on-reads.md)
- [x] Fail-closed proven on reads: no-policy deny, dangling-policy deny, anonymous sees only Anyone-granted, and a policy-denied read is a structured secret-free refusal (#20260724013200-fail-closed-and-structured-denial-on-reads.md)

## Changelog

- 2026-07-24 — mission created from the bare /mission planning session (successor to a-request-resolves-to-a-principal; strategy gap after its close) — mission.md
- 2026-07-24 — strategy linked — access-derives-from-the-resolved-principal
- 2026-07-24 — ruling recorded: enforcement only; the "what may I administer" product decision stays parked (developer, 2026-07-24) — mission.md
- 2026-07-24 — ticket added — 20260724013000-enforce-policy-on-the-serve-read-path.md
- 2026-07-24 — ticket added — 20260724013100-every-grant-axis-bites-on-reads.md
- 2026-07-24 — ticket added — 20260724013200-fail-closed-and-structured-denial-on-reads.md
- 2026-07-24 — drive_authorized stamped after the creation interrogation (scope ruling above; per-ticket Policies and Quality Gate pre-answered) — mission.md
- 2026-07-25 — ticket archived — 20260724013000-enforce-policy-on-the-serve-read-path.md
- 2026-07-25 — ticket archived — 20260724013100-every-grant-axis-bites-on-reads.md
- 2026-07-25 — ticket archived — 20260724013200-fail-closed-and-structured-denial-on-reads.md
- 2026-07-25 — run recorded (+0.89h) — 20260725-101714
- 2026-07-25 — story reported — work-20260724-011032.md

- 2026-07-28 — concern deferred (stuck) — the-statement-bridge-s-read-leg.md
- 2026-07-28 — mission achieved — mission.md
## Reflection

### 2026-07-25 run 20260725-101714
- blocked: nothing stopped autonomy — the three-ticket queue drained, the full gate stayed green, and the four pre-release corrections were applied in-worktree without an escalation.
- leaked questions: which policy governs the statement bridge POST /api/run, which has no policy field to resolve and whose answer touches the parked what-may-I-administer ruling; and whether MINOR is the right plugin bump for a taught-surface break that turns a shipped cookbook recipe into a 403.
- front-load next: when a mission turns a default-deny on, pre-authorize a sweep of the in-repo claims it will falsify — four of this branch's defects were a taught prose spelling, a fixture comment, and two blueprint statements, none of which any test could catch; and rule the non-endpoint serve faces' policy source before the next mission touches one.
