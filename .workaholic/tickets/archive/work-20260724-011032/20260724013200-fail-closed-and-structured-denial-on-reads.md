---
created_at: 2026-07-24T01:32:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 1h
commit_hash:
category: Changed
depends_on: [20260724013000-enforce-policy-on-the-serve-read-path.md]
mission: what-a-principal-can-see-and-do-is-granted-by-policy
---

# Fail-closed and structured denial on reads

## Overview

Two proof surfaces for the newly-enforced read path:

1. **Fail-closed matrix.** No attached policy ⇒ deny; dangling policy ref ⇒ deny
   (`resolve_policy`, `crates/server/src/policy/gate.rs`); anonymous actor ⇒ only
   `Subject::Anyone` + `Condition::Always` rules match (so an anonymous read of a
   FOR/WHERE-narrowed grant is denied while an `ALLOW SELECT FOR anyone` read succeeds — the
   signed-out state stays a first-class, non-error answer where granted). An unknown effect kind
   stays denied (`classify_effect` → Unknown ⇒ deny). Nothing widens: a test pins that enabling
   read enforcement did not change any write-side decision.
2. **A denial is a structured answer.** A policy-denied read surfaces to the serve caller as a
   structured, secret-free refusal carrying the `deny_reason` shape
   (`crates/server/src/policy/enforce.rs` `PolicyDecision`) — never an empty relation at exit 0,
   and never an error text that leaks rule internals or credential material. The
   record matches the honesty rule the sibling predicate mission enforces for `where`.

## Policies

- workaholic:safety — fail-closed is permanent: an unresolved or unrecognized actor gets least
  privilege; no threading of identity ever widens a default. Deny reasons are secret-free.
- workaholic:design / 「推測するな、宣言して拒否せよ」 — a read the policy does not admit is
  refused with a structured answer, never silently emptied.

## Quality Gate

1. Hermetic tests: no-policy deny, dangling-ref deny, anonymous-vs-Anyone both directions, and
   the no-widening regression (a write-side decision matrix snapshot unchanged before/after).
2. A serve-level test asserts the denied read's response shape: structured refusal, secret-free,
   distinguishable from an empty result — the assertion checks both the status and the absence of
   a `rows` payload.
3. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check` if a described surface moved.

## Considerations

- The existing secret-shape scan in `record-evidence`-style tooling is unrelated; the secret-free
  guarantee here is `PolicyDecision::deny_reason`'s existing contract — keep it, assert it.
- If the structured-denial shape needs a serve-face representation (HTTP status + body), follow
  the existing structured-error conventions in `crates/http` rather than inventing one.

## Final Report

Development completed as planned. Fail-closed is proven on the read path in all three resolution
shapes, and a denied read is now provably a structured refusal rather than an empty relation.

### Discovered Insights

- **Insight**: the no-widening proof is best written as a FROZEN decision matrix rather than a
  prose claim — nine `(policy, plan) -> allowed?` rows whose verdicts predate read enforcement.
  The two rows that actually catch the plausible regressions are "a SELECT-only grant grants NO
  write" and "a write's Read dependency is not gated, even on an ungranted driver": those are the
  exact two ways read enforcement could have leaked into the write side.
  **Context**: extend the matrix rather than adding scattered one-off assertions.
- **Insight**: `EffectClass::Unknown` is unconstructible today because `classify_effect`'s match is
  total over the known `EffectKind` variants. That is the property worth pinning (the residual arm
  exists so a FUTURE variant starts denied), so the test asserts totality rather than pretending to
  exercise a branch it cannot reach.
  **Context**: an honest test of an unreachable fail-closed branch is a totality test.
- **Insight**: "distinguishable from an empty result" has to be asserted against a real granted
  response in the same test, not assumed. The refusal is 403 with `error: policy` and no `rows` or
  `schema` key; the granted read is 200 with a `rows` envelope. Asserting only the refusal's shape
  would not prove the two are unconfusable.
