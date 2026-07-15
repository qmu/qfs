---
created_at: 2026-07-09T10:42:55+09:00
author: a@qmu.jp
type: refactoring
layer: [Domain]
effort: 1h
commit_hash:
category: Changed
depends_on: [20260709104254-blueprint-type-system-chapter.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Two-layer model + stage admission test — blueprint principle and language-reference narration

## Overview

Write down the design principle the whole mission clarified: qfs is a **two-layer language** — a
closed, first-order relational **stage algebra** (planable, pushdown-capable, effect-gated) over a
**total, pure, row-scoped expression layer** where functions are values. Every stage is notation
for an implicit-lambda combinator application (`where p ≡ filter(rel, (row) => p)`); the stage
layer stays closed because the planner and the preview/commit gate must *see through* every stage.

This ticket produces two artifacts from that principle:

1. A blueprint section stating the **stage admission test** — a construct may become a pipe stage
   only if the planner or the gate must see through it: (a) pushdown-translatable, (b) plan-time
   schema rewrite, (c) effect gating, (d) cardinality/ordering semantics. Everything else is a
   stdlib function (expression layer) or data under a path. It records that all 39 keywords and
   every current `PipeOp` pass the test, and instructs future stage proposals to cite it. The
   test's formal content ("every stage has a typing rule") is inherited from the type-system
   chapter (`depends_on`).
2. The corrected **self-description** in the generated language reference: qfs is "a closed
   relational pipe algebra + a total pure expression language with functions-as-values + declared
   effect seams," with the desugaring **equivalence table** (`where p ≡ filter(rel, (row) => p)`,
   `select`, `order by`, etc.) so special forms read as notation, not exception. Because
   `docs/language.md` is generated, the prose lives in the `qfs-lang` source that gen-docs renders.

Merges mission acceptance items "stage admission test + two-layer self-description" and
"docs/language.md two-layer model + equivalence table" — one coherent write-down of the layering.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the reference prose must be
  edited at its gen-docs source, never in the generated `docs/language.md`.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to the `qfs-lang` source
  doc-comment/string edits.
- `workaholic:implementation` / `policies/functional-programming.md` — the equivalence table IS
  the FP framing (special forms as combinator applications); the narration must be faithful to it.
- `workaholic:implementation` / `policies/objective-documentation.md` — the admission test's
  (a)-(d) criteria must be stated as checkable predicates, and the "all keywords/PipeOps pass"
  claim must be verifiable against the inventory.

## Key Files

- `docs/blueprint.md` - the stage-admission-test section (near §2/§3 grammar, referencing the
  type chapter for the per-stage typing rule).
- `packages/qfs/crates/lang/src/*` (the gen-docs source of `docs/language.md` intro/grammar
  prose) - the two-layer self-description and the equivalence table; `keywords.rs`/`reference.rs`
  hold the rendered narration.
- `docs/language.md` - regenerated output; never hand-edited (asserted by gen-docs `--check`).
- `packages/qfs/crates/parser/src/ast.rs` - `PipeOp` variants, the inventory the admission-test
  section enumerates as passing.

## Related History

- [20260622214650-t04-grammar-ast-and-governance.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t04-grammar-ast-and-governance.md) - the governance lock on `PipeOp`/keywords the admission test formalises
- [20260627120100-t74-lowercase-keywords.md](.workaholic/tickets/archive/work-20260628-000332/20260627120100-t74-lowercase-keywords.md) - the 39-keyword freeze the test records as passing
- [20260626101900-t61-lambdas-higher-order-fns.md](.workaholic/tickets/archive/work-20260628-000332/20260626101900-t61-lambdas-higher-order-fns.md) - "functions are values" (decision H), the expression-layer half of the model

## Implementation Steps

1. Write the blueprint stage-admission-test section: the (a)-(d) criteria, the "else → stdlib fn
   or path-data" rule, and an inventory note that the 39 keywords + every `PipeOp` variant satisfy
   it. Reference the type chapter for "every stage has a typing rule."
2. In the `qfs-lang` gen-docs source, replace the "higher-order function-pipeline" self-framing
   with the two-layer description, and add the stage↔combinator equivalence table
   (`where`/`select`/`extend`/`order by`/`limit`/`distinct` → their `filter`/`map`/… readings).
3. Regenerate: `cargo run -p xtask -- gen-docs` and commit the updated `docs/language.md`.
4. Verify no drift and tick the two mission acceptance boxes with this ticket's filename.

## Quality Gate

**Acceptance criteria:**
- Blueprint states the admission test with the four criteria and the inventory-passes note.
- `docs/language.md` (regenerated) carries the two-layer self-description and the equivalence
  table; the stale "higher-order function-pipeline" self-claim is gone.
- `docs/language.md` matches the binary (no gen-docs drift).

**Verification method:**
- `cd packages/qfs && cargo run -p xtask -- gen-docs --check` green (proves the committed doc
  matches the source).
- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- `cargo run -p xtask -- gen-skills --check` green.
- Owner reads the admission-test section and the equivalence table and confirms the framing.

**Gate:** gen-docs `--check` + full anti-drift suite green + owner read-through; mission boxes updated.

## Considerations

- The equivalence table is documentation of an existing semantics, not a new feature — do not
  introduce a real `filter(rel, …)` over relations here (relation-level higher-order is the
  separate pipeline-valued-lambdas decision ticket).
- Depends on the type chapter for the per-stage typing-rule vocabulary; do not restate types here.
- `docs/language.md` is generated — editing it directly will pass locally but fail CI drift.

## Final Report

Development completed as planned. The blueprint now states the two-layer model explicitly: a closed
relational stage algebra over typed paths, above a total, pure, row-scoped expression layer where
functions are values. It also records the stage admission test: a construct may become a pipe stage
only if the planner or gate must see through it for pushdown translation, schema rewriting, effect
gating, or cardinality/ordering semantics; otherwise it belongs in the expression layer or under a
path as data.

The generated language reference now carries the same self-description and a stage-reading table
(`where p` as `filter(rel, (row) => p)`, `select` as projection, `extend`/`set` as row maps, and so
on). That prose lives in `qfs-lang::reference` as `language_model_reference()` and is re-exported
through `qfs-core`, so `docs/language.md` stays generated from language-owned source rather than
hand-edited markdown. A renderer test locks the two-layer prose and equivalence table into the
generated reference.

### Discovered Insights

- **Insight**: `docs/language.md` had generated prose hard-coded in `qfs::docs`, even though the
  banner only named the qfs-lang vocabulary and grammar as sources.
  **Context**: Moving the language model prose into `qfs-lang::reference` makes the generated-doc
  provenance true and keeps future language narration beside the frozen vocabulary and EBNF.
- **Insight**: The stage admission test is already latent in the `PipeOp` governance count.
  **Context**: The current 19 `PipeOp` variants each satisfy at least one admission criterion, so
  future stage proposals can be reviewed against the same closed-core invariant instead of taste.
