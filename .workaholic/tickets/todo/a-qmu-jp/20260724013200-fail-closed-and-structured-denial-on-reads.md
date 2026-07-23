---
created_at: 2026-07-24T01:32:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
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
