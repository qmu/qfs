---
created_at: 2026-07-09T10:43:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 2h
commit_hash: e75c1cd
category: Added
depends_on: [20260709104256-reference-convention-transform-surface.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Transform one-seam lock — assert `transform` is the only model-call seam (plan check + visibility)

## Overview

Turn blueprint §15's one-seam thesis — "qfs MAY make an authenticated outbound model call, but
**only** through the `transform` seam; no other statement, driver, or code path calls a model" —
from prose into an enforced invariant. Per the owner's quality-gate answer, enforce it **two
ways** (plan-level test **and** visibility):

1. **Plan-level test.** A test asserts that a model-call effect node in a `Plan` originates
   **only** from `PipeOp::Transform`. Construct plans for a spread of statements (reads, writes,
   `call`, codecs, DDL) and assert none carries a model-call effect; construct a transform-bearing
   statement and assert it does. This is the governance lock that fails if a future change routes a
   model call through any other stage.
2. **Visibility restriction.** The `ModelProvider` seam (introduced by the T2/T3 execution ticket
   `20260708192732-transform-execution-routing.md`) is reachable **only** from the transform
   applier — enforced by Rust module visibility (`pub(crate)`/sealed trait / crate boundary) so no
   other driver or code path can obtain a provider to call a model. A compile-time guarantee
   complements the runtime/plan test.

Depends on the reference-convention/transform-surface ticket (the stage surface must be settled)
and coordinates with the transform execution ticket (which introduces `ModelProvider`). If this
ticket is driven before execution lands, deliver the plan-level test now and the visibility lock as
a follow-up note in the execution ticket; if after, deliver both.

## Policies

- `workaholic:implementation` / `policies/type-driven-design.md` — make the illegal state
  unrepresentable: a model call from outside the transform applier should not **compile**, not
  merely fail a test (the visibility half).
- `workaholic:implementation` / `policies/directory-structure.md` — the `ModelProvider` trait and
  applier live in the driver-claude-shaped leaf; the visibility boundary is a crate/module rule.
- `workaholic:implementation` / `policies/functional-programming.md` — a model call is an effect;
  the lock guarantees it only ever enters via the one declared, gated effect node.
- `workaholic:implementation` / `policies/objective-documentation.md` — the §15 one-seam claim
  becomes a checkable test, not an aspiration.
- `workaholic:design` / `policies/consent-recording.md` — the single seam is what makes the
  model call previewable and consent-gated; the lock protects that safety property.

## Key Files

- `packages/qfs/crates/exec/src/*` (or `qfs-plan`) - where a `Plan`'s effect nodes are built; the
  plan-level test asserting model-call effects trace to `PipeOp::Transform` only.
- `packages/qfs/crates/parser/src/ast.rs` - `PipeOp::Transform`, the sole legal origin.
- `packages/qfs/crates/driver-claude/src/*` - the applier/`ModelProvider` seam whose visibility is
  sealed to the transform path (coordinate with the execution ticket).
- `docs/blueprint.md` §15 - cross-reference the enforced lock from the one-seam prose.

## Related History

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) - the §15 brief that states the one-seam thesis (decision W reverses decision K's absolute)
- [20260622214650-t04-grammar-ast-and-governance.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t04-grammar-ast-and-governance.md) - the `PipeOp` governance-lock pattern this test extends to effect origin

## Implementation Steps

1. Add a plan-level test enumerating representative non-transform statements and asserting no
   model-call effect node appears in their plans; assert a transform-bearing statement's plan does.
2. Seal the `ModelProvider` seam's visibility so only the transform applier can reach it
   (`pub(crate)` / sealed trait / crate boundary); add a compile-fail or a doc-test demonstrating an
   external caller cannot construct/invoke a provider.
3. Cross-reference the enforced lock from blueprint §15.
4. Coordinate with `20260708192732-transform-execution-routing.md`: if `ModelProvider` does not
   yet exist, land the plan-level test and record the visibility lock as a required check in the
   execution ticket's Considerations.
5. Full suite; tick the mission's one-seam acceptance box.

## Quality Gate

**Acceptance criteria:**
- A test proves model-call effect nodes originate only from `PipeOp::Transform` (non-transform
  statements: zero; transform statement: one).
- The `ModelProvider` seam is not reachable outside the transform applier (visibility-enforced;
  demonstrated by a compile-fail/doc-test or module-boundary assertion).
- Blueprint §15 cites the enforced lock.

**Verification method:**
- `cd packages/qfs && cargo test --workspace` (the plan-origin test passes; the visibility
  compile-fail/doc-test behaves as specified).
- `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`.
- `cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` green.
- Owner confirms both the plan check and the visibility restriction are in place (or the split
  with the execution ticket is agreed).

**Gate:** both enforcement mechanisms green (or plan-test now + visibility tracked in the execution
ticket, with owner agreement); mission box updated.

## Considerations

- Timing coupling with the transform execution ticket (`ModelProvider` origin): sequence so the
  visibility lock lands with or immediately after the seam is introduced — never a window where a
  provider exists unsealed.
- The plan-level test must use hermetic mock providers (§11 `MockHttp` posture) — it asserts plan
  **shape**, never makes a network call.
- Keep the lock at the effect-node/plan layer, not the executor, so it catches a mis-routed model
  call at plan time (before any commit), consistent with the safety floor.
