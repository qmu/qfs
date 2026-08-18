---
created_at: 2026-08-16T19:10:00+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260818-212751
---

# A computed projection still answers nulls for a column that does not exist

## Overview

`20260725113000` made a **name-only** `select` refuse a column the relation does not carry. The
sibling form — a projection that **renames or computes** (`select nosuch AS x`, a struct/array
constructor over an absent column) — still answers, one `null` per row, at exit 0.

Measured 2026-08-16 on `qfs 0.0.103` (the build that carries the name-only refusal):

```
$ qfs run "/local/tmp/qfsdemo |> select nosuchcol as x |> limit 2"
{"schema":[{"name":"x","type":"unknown"}],"rows":[{"x":null},{"x":null}],"meta":{"row_count":2,...}}
EXIT=0

$ qfs run "/local/tmp/qfsdemo |> select nosuchcol |> limit 2"
{"error":{"code":"unknown_column","kind":"usage","message":"`select` names column 'nosuchcol', …"}}
EXIT=2
```

Two spellings of one question now answer two different ways, and the *forgiving* one is the one an
operator reaches for when they want to name the output column.

## Mechanism

- `packages/qfs/crates/pushdown/src/lower.rs` (`PipeOp::Select`) routes a projection to
  `LogicalPlan::Project` only when every term `is_plain_projection`; a rename or a computed term
  lowers to `LogicalPlan::ProjectExpr` carrying per-row `ScalarExpr`s.
- `20260725113000`'s two refusals both sit on the **`Project`** road: `planner::check_project_columns`
  (plan time) and `engine::eval::project_checked` (runtime). `ProjectExpr` passes neither.
- `packages/qfs/crates/engine/src/eval.rs` — `eval_value` resolves `ScalarExpr::Col` with
  `resolve(...).unwrap_or(Value::Null)`. That total shape is deliberate for `EXTEND`/`SET` over a
  late-bound row; whether it is right for a caller-written **projection** is this ticket's question.

## Scope

**In scope:** deciding, and then implementing, whether a `ScalarExpr::Col` in a caller-written
`SELECT` that names a column absent from a **described, non-empty** relation is a structured
`unknown_column` refusal (`kind: usage`, exit 2) or stays a `null`.

**Out of scope:**

- `EXTEND`/`SET` and the switch arms' own projections — they share `eval_value`, and widening the
  refusal to every consumer of it is a different, larger question.
- The name-only path — settled and shipped by `20260725113000`.
- The late-bound leniency: an empty schema is never refused, whichever way this lands.

## Open Decisions

- **Does a computed/renamed projection refuse too?** Refusing is consistent with the name-only
  sibling and with `where`/`expand`, and makes the two spellings answer alike. Keeping the null is
  defensible for a genuinely heterogeneous relation, and cheaper: `eval_value` is shared with
  `EXTEND`, so the check has to be applied at the projection site rather than inside it. A run can
  gather the evidence but the consistency-vs-leniency call is the developer's, exactly as it was for
  the name-only form (ruled 2026-07-26).

## Key Files

- `packages/qfs/crates/pushdown/src/lower.rs` — `is_plain_projection` / `project_expr_terms`, the
  fork that sends a rename down the unchecked road.
- `packages/qfs/crates/pushdown/src/planner.rs` — `check_project_columns`, the shape a
  `ProjectExpr` check would follow (it has the input schema in hand at the `ProjectExpr` arm too).
- `packages/qfs/crates/engine/src/eval.rs` — `project_expr` / `eval_value`.
- `packages/qfs/crates/exec/tests/oneshot.rs` — `select_refuses_an_unknown_column`, the module a
  computed case would join.

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — a column that does not exist is a
  malformed question in either spelling; answering `null` guesses what the caller meant.
- `workaholic:implementation` / `objective-documentation` — `docs/cookbook/faq.md` now says a
  `select` naming an absent column refuses; today that is true of one spelling only.

## Quality Gate

**Acceptance criteria**

1. The decision is recorded with its reason in the ticket outcome and the commit body.
2. If refusal: the two commands above answer alike, with the actual output and raw exit codes
   pasted; a both-directions test covers the rename and a computed constructor.
3. A late-bound / undescribable relation is still not refused, pinned by a test.
4. `EXTEND`/`SET` are unchanged, pinned by a test.

**Verification method**

- The commands above re-run against the built binary with `echo "EXIT=$?"` pasted, not paraphrased.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check` / `gen-skills --check` all exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-16 while driving `20260725113000`, whose
  scope was explicitly the name-only projection (its measurement, its Key Files and its ruling all
  name `project`, never `project_expr`). Reported there as a concern rather than folded in: the
  ruling that settled the name-only form was written against the evidence for that form, and a run
  may not extend a developer's decision to a road they did not measure.
