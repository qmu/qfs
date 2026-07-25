---
created_at: 2026-07-24T01:30:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: what-a-principal-can-see-and-do-is-granted-by-policy
---

# Enforce policy on the serve read path

## Overview

The policy gate is inert on data reads: `crates/server/src/policy/enforce.rs` classifies only
write/CALL effects (`classify_effect` skips `Read`/`List`), a pure read lowers to an empty commit
plan, and `evaluate`/`evaluate_with_context` return Allow regardless of policy — pinned today by
`select_only_plan_is_allowed_even_under_empty_policy` (enforce.rs). The serve pipeline
(`crates/http/src/handler.rs` `dispatch_inner` → `crates/http/src/policy.rs`
`assert_read_only(plan, policy, actor)`) therefore passes every read, even though the resolved
`DecisionContext` is already in hand and the `SELECT` verb, `ScopeGlob`, `Subject`, `Condition`,
and `RoleGraph` machinery all exist (`crates/server/src/policy/model.rs`).

Make the serve read path evaluate `SELECT` against the endpoint's resolved policy with the real
actor. The predecessor mission threaded "who is asking" to this exact seam; this ticket makes the
answer matter.

## Implementation direction

- Extend the enforcement classification so a read plan's scan targets are evaluated as `SELECT`
  effects against the policy's rules (first-matching-rule semantics, `policy.default` else),
  using the scan's path for the `AT` ScopeGlob axis. The evaluation happens where
  `assert_read_only` already sits in `dispatch_inner`, BEFORE the read executes.
- `resolve_policy` semantics stay: endpoint with no policy ref → default-deny only if that is the
  current write-side behavior for that endpoint shape; the fail-closed matrix is ticket
  20260724013200's proof surface — do not widen any default here.
- The CLI/local path (`SafetyMode` floor, loopback super-admin in `crates/qfs/src/sys.rs`) is out
  of scope; only the serve faces change.
- The existing pinned test `select_only_plan_is_allowed_even_under_empty_policy` asserts the OLD
  behavior — flip/replace it deliberately as part of this ticket (it is the ratchet for the
  inert-on-reads state, and its inversion is the proof the state is gone). qfs is experimental:
  this is a sanctioned hard break, no compatibility shim.

## Policies

- workaholic:implementation / machine-checkable-domain-gaps — the gate's decision must be a typed
  evaluation over the plan, not prose; the flipped test is the checkable artifact.
- workaholic:design / アクセス制御 — one policy governs every face; the read seam is the last face
  where the policy did not bite.
- workaholic:safety — fail-closed: nothing in this ticket may widen an existing default; deny
  reasons stay secret-free (`deny_reason` shape).

## Quality Gate

1. A serve request against an endpoint whose policy has `ALLOW SELECT` on the scanned path
   succeeds; the same request with `DENY SELECT` (or no matching rule under `default: Deny`) is
   refused BEFORE the driver scan runs — both directions proven by hermetic tests at the
   `crates/http` level using the real `evaluate_with_context`.
2. `select_only_plan_is_allowed_even_under_empty_policy` is inverted/replaced: an empty policy
   under `default: Deny` now denies the read on the serve path, and the test name says so.
3. The CLI/local read path behavior is unchanged (existing CLI tests stay green untouched).
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check` if a described surface moved.

## Considerations

- The write/CALL evaluation path (`gate_plan_with_context`, fire-path committers) is already
  correct — reuse `rule_matches_in_context`; do not fork a read-specific matcher.
- Watch the dep-direction guard (`crates/cmd/tests/dep_direction.rs`) — the evaluation stays in
  `qfs-server`/`qfs-http`, never leaks into `qfs-exec`.

## Final Report

Development completed as planned. The serve read path now evaluates SELECT against the endpoint's
resolved policy under the request's real actor, before any driver scan runs.

### Discovered Insights

- **Insight**: A pure read produces an EMPTY plan, not a plan of `Read` nodes. `build_plan` returns
  `Plan::pure()` for a `SELECT` (`crates/exec/src/exec.rs`), so the inert-on-reads state was not
  "the enforcer skips `EffectKind::Read`" — it was "there is nothing for the enforcer to look at".
  Making `classify_effect` map `Read` to `SELECT` would therefore have gated every WRITE's read
  dependency (changing the write-side decision matrix) while still leaving pure reads ungated. The
  fix had to be a second entry point over the read's scan targets, derived from the physical plan.
  **Context**: anyone reaching for "just classify Read as SELECT" will get the blast radius exactly
  backwards.
- **Insight**: the request-time gate looked the endpoint's policy up by ENDPOINT NAME
  (`policies.get(&route.name)`), while registration looked it up by the endpoint's `policy:` REF.
  A policy not named after its endpoint silently applied at registration and not at request time.
  Fixed by carrying the ref on `CompiledRoute` and resolving through `qfs_server::resolve_policy`.
  **Context**: this was invisible while reads were ungated and writes were re-gated identically at
  registration; enabling read enforcement is what made the divergence observable.
- **Insight**: `qfs-exec::scan_targets` deliberately shares `execute_read_with`'s planning steps
  (codec-tail strip + `plan_query`) rather than re-deriving them. If the two ever drift, the gate
  adjudicates a different set of paths than the driver is asked for — a silent authorization hole.
  **Context**: keep them adjacent in `exec.rs`, and never re-implement target derivation elsewhere.
- **Insight**: the shipped boot fixture `crates/server/fixtures/server_boot.qfs` carried a comment
  asserting the opposite of this mission ("a pure READ produces no commit plan, so it needs no
  POLICY"). Fixture prose is a specification a lot of tests count against — changing it moved four
  independent audit-entry-count assertions.
  **Context**: audit counts in `qfs-server`/`qfs-cmd` tests are keyed to that fixture's statement
  count; adding a statement to it means updating them together.
