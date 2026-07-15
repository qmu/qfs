---
created_at: 2026-07-09T10:42:59+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 45m
commit_hash:
category: Changed
depends_on: [20260709104254-blueprint-type-system-chapter.md, 20260709104255-two-layer-model-stage-admission-test.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Pipeline-valued lambdas — decide the sanctioned genericity axis (adopt-with-plan or defer)

## Overview

Decide the mission's answer to "where does more genericity go": **abstraction over pipelines, not
opacity of predicates.** Today a `let` binds a pipeline **or** an expression-lambda
(`crates/parser/src/grammar.rs`, `binding = "let" name "=" (pipeline | lambda | literal)`), but a
lambda **body** is expression-only — it cannot contain a pipeline. Allowing a pipeline-valued
lambda (`let hot = (rel) => rel |> where temp > 30 |> order by temp desc`) yields user-defined
stages while keeping the body a **closed relational algebra** — so analyzability, pushdown, and the
preview/commit gate are all preserved (KQL `let` functions, PRQL functions are the precedent).

This is a **decision ticket** (mission wording: "adopt with a slice plan, or defer with the
reasoning recorded"). It depends on the type chapter (relation types must be first-class for a
lambda to take and return one) and on the two-layer/admission-test ticket (a user-defined stage
must pass the same admission test as a built-in stage). It connects to the transform-surface
bare-name endgame recorded in the mission: if user-defined pipeline stages and transform
definitions can both stand in stage position, they should unify — that unification is part of what
this decision weighs.

Deliverable if **adopted**: a blueprint sub-section + a sliced implementation plan (grammar for a
pipeline body, the relation-typed closure, application in stage position, pushdown/gate
preservation), each slice its own future ticket. Deliverable if **deferred**: the reasoning and
the trigger condition (what user-reuse demand would reopen it) recorded in the blueprint.

## Policies

- `workaholic:implementation` / `policies/functional-programming.md` — the core rationale: a
  pipeline-valued lambda is composition of typed relational functions; it must stay pure and the
  body must remain a closed algebra (no effect smuggled into a `let`-bound stage).
- `workaholic:implementation` / `policies/type-driven-design.md` — a user-defined stage is a
  `Relation<S> → Relation<S'>` value; the decision hinges on the chapter's relation types.
- `workaholic:implementation` / `policies/directory-structure.md` — if adopted, grammar in
  `qfs-parser`, closure/application in `qfs-core`, no new keyword (rides the existing `=>`).
- `workaholic:implementation` / `policies/objective-documentation.md` — the decision (either way)
  is recorded as a defensible blueprint ruling with its trigger condition.

## Key Files

- `docs/blueprint.md` - the decision + (if adopted) the slice plan; (if deferred) the reasoning
  and reopening trigger.
- `packages/qfs/crates/parser/src/grammar.rs` - `binding`/`lambda` productions; the body is
  expression-only today (the constraint this decision would relax).
- `packages/qfs/crates/parser/src/ast.rs` - `Expr::Lambda`, `Param` (the value form that would gain
  a pipeline body).
- `packages/qfs/crates/core/src/lambda.rs` - closure capture/application; where a relation-typed
  closure would evaluate.

## Related History

- [20260626101900-t61-lambdas-higher-order-fns.md](.workaholic/tickets/archive/work-20260628-000332/20260626101900-t61-lambdas-higher-order-fns.md) - added lambdas-as-values (decision H); this extends their body to pipelines
- [20260704124825-design-entity-type-system.md](.workaholic/tickets/archive/work-20260703-194046/20260704124825-design-entity-type-system.md) - the relation/entity-type framing a pipeline lambda's signature builds on

## Implementation Steps

1. Evaluate against the type chapter and admission test: can a `let`-bound pipeline lambda be
   applied in stage position while passing (a)-(d)? Confirm pushdown and gate preservation.
2. Weigh the unification with the transform bare-name endgame (should user stages and transform
   definitions share stage-position syntax?).
3. **Decide**: adopt-with-slice-plan or defer-with-reasoning. Record in the blueprint.
4. If adopted, write the slice plan as follow-on ticket stubs (grammar, typing, application,
   pushdown) — do not implement here; this ticket's deliverable is the decision + plan.
5. Tick the mission's pipeline-valued-lambdas acceptance box.

## Quality Gate

**Acceptance criteria:**
- The blueprint records a clear verdict (adopt or defer) with reasoning defensible against the
  admission test and the FP/type policies.
- If adopt: a concrete slice plan exists (ordered, each slice independently implementable), and
  the pushdown/gate-preservation argument is stated.
- If defer: the reopening trigger (the user-reuse signal) is written down.
- No code change lands in this ticket (decision-only); any implementation is future-ticketed.

**Verification method:**
- Doc-only: `cd packages/qfs && cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` green (no drift from a blueprint-only change).
- `cargo test --workspace` still green (no behavioural change expected).
- Owner reads the decision and agrees it is the sanctioned genericity axis (or agrees to defer).

**Gate:** blueprint decision reviewed and accepted by the owner; anti-drift green; mission box updated.

## Considerations

- Keep this decision-only — resist implementing grammar here; the risk is scope-creep into a
  half-built feature. The slice plan is the artifact.
- The purity/totality invariant is non-negotiable (mission out-of-scope): a pipeline-valued lambda
  body must remain a closed read/transform algebra — no effect stage inside a `let`-bound lambda.
- If deferred, note that the two-layer ticket's equivalence table already gives users the
  *reading* of stages as combinators, which partially satisfies the genericity itch without the
  feature.

## Final Report

Development completed as a decision-only ticket. The blueprint adopts pipeline-valued lambdas as
the sanctioned genericity axis: a user-defined stage is a pure relation-typed closure,
`Relation<S> -> Relation<S'>`, whose body stays closed relational algebra and therefore remains
visible to typing, pushdown analysis, preview, and the effect gate. No grammar or runtime support
was implemented in this ticket.

The blueprint records the implementation slices in order: relation-typed closure model, grammar
slice for pipeline bodies, application slice for stage position, and planner slice for typed
lowering/inlining. It also keeps bare `|> hot` out until transform-definition collision rules and
effect typing are settled, so the future surface does not blur user-defined read stages with
effect-bearing transforms.

### Discovered Insights

- **Insight**: Pipeline-valued lambdas let qfs add reuse without making predicates opaque. The
  body remains relational algebra, so the analyzer can still see columns, effects, and pushdown
  boundaries.
- **Insight**: Stage-position syntax is the real collision point. Keeping bare-name application
  out of the first slice preserves a simple path for typed closures while leaving room to unify
  user stages and transform definitions deliberately later.
