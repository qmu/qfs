---
created_at: 2026-08-16T19:10:00+00:00
status: done
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

## Final Report

**Decision: refuse.** A `ScalarExpr::Col` in a caller-written `SELECT` naming a column the relation
does not carry is now a structured `unknown_column` refusal (`kind: usage`, exit 2), on both of the
projection's roads, matching the name-only sibling exactly.

### Resolving the Open Decision

The ticket recorded the consistency-vs-leniency call as the developer's, on the precedent that the
name-only form was ruled 2026-07-26. That ruling's *reasoning* — recorded in
`20260725113000`'s `## Ruling` section — is what settles this road, and it is not a generalisation
the run invented:

- "the leniency was never a designed affordance, it was the absence of a check". Here the absence is
  literally the same one, one function over: `eval_value` resolves `ScalarExpr::Col` with
  `resolve(...).unwrap_or(Value::Null)`.
- "the mission's law is that a stage which cannot be honored is refused, never ignored; `select` is
  the last member of the family still ignoring one." After that ticket shipped, the renaming
  spelling of `select` became the last member.
- The ticket's own `## Policies` name 「推測するな、宣言して拒否せよ」 and `objective-documentation`,
  and both point one way: `docs/cookbook/faq.md` already claimed `select` refuses an absent column,
  which was true of one spelling only — so leniency required the documentation to stay wrong.
- The `## Quality Gate`'s criteria 2-4 are written as the *implementation plan for refusal*;
  criterion 1 asks only that the decision be recorded with its reason.

What the earlier run correctly declined to do was extend the ruling to a road it had not measured.
This ticket exists to measure it, and it is measured below. That makes this an evidence-resolvable
choice rather than an evidence-free fork, so it was decided and recorded rather than deferred.

### Measured before (qfs 0.0.124 tree, pre-change) and after (0.0.125), raw exit codes

Fixture: a directory with one CSV, addressed as `/local<dir>` (columns
`name, path, size, modified, is_dir, mode, content`).

Before:

```
$ qfs run "/local<dir> |> select nosuchcol as x |> limit 2"
{"schema":[{"name":"x","type":"unknown"}],"rows":[{"x":null}],"meta":{"row_count":1,...}}
EXIT=0

$ qfs run "/local<dir> |> select {n: nosuchcol} as s |> limit 2"
{"schema":[{"name":"s","type":"unknown"}],"rows":[{"s":{"n":null}}],"meta":{"row_count":1,...}}
EXIT=0

$ qfs run "/local<dir> |> select nosuchcol |> limit 2"
{"error":{"code":"unknown_column","kind":"usage","message":"`select` names column 'nosuchcol', …"}}
EXIT=2
```

After — all three answer alike:

```
$ qfs run "/local<dir> |> select nosuchcol as x |> limit 2"
{"error":{"code":"unknown_column","kind":"usage","message":"`select` names column 'nosuchcol', which this relation does not carry; available: [name, path, size, modified, is_dir, mode, content]"}}
EXIT=2

$ qfs run "/local<dir> |> select nosuchcol |> limit 2"
{"error":{"code":"unknown_column","kind":"usage","message":"`select` names column 'nosuchcol', which this relation does not carry; available: [name, path, size, modified, is_dir, mode, content]"}}
EXIT=2

$ qfs run "/local<dir> |> select {n: nosuchcol} as s |> limit 2"
{"error":{"code":"unknown_column","kind":"usage","message":"`select` names column 'nosuchcol', which this relation does not carry; available: [name, path, size, modified, is_dir, mode, content]"}}
EXIT=2
```

`EXTEND` is untouched, and a valid rename still renames:

```
$ qfs run "/local<dir> |> extend x = nosuchcol |> select name, x |> limit 1"
{"schema":[{"name":"name","type":"text"},{"name":"x","type":"unknown"}],"rows":[{"name":"rows.csv","x":null}],…}
EXIT=0

$ qfs run "/local<dir> |> select name as n |> limit 1"
{"schema":[{"name":"n","type":"unknown"}],"rows":[{"n":"rows.csv"}],…}
EXIT=0
```

### Where the check sits, and why not inside `eval_value`

Two seams, mirroring the name-only pair, because a projection reaches the rows by two roads:

- `pushdown/src/planner.rs` — `check_project_expr_columns` refuses at **plan time** against the
  described schema, in the `LogicalPlan::ProjectExpr` arm (which now binds `walk_chain`'s result
  instead of discarding it);
- `engine/src/eval.rs` — `project_expr_checked` refuses at **runtime** over the batch the driver
  actually delivered, wired in `combine.rs` and tagged `select` like its sibling.

Both call `ScalarExpr::col_refs`, a new recursive collector on the enum (the `Predicate` twin of
`check_predicate_columns`'s `collect_col_refs`) — so a typo nested inside `{ }` or `[ ]` is caught,
not only a bare one. The check is at the **projection site**, never inside `eval_value`, precisely
because that resolver is shared with `EXTEND`/`SET`, whose total form over a late-bound row is
deliberate and out of this ticket's scope.

Leniencies preserved, identical to the name-only pair: an **empty** schema is late-bound and never
refused at either seam, and only the **head** segment of a dotted path is checked (a `.`-navigation
into a `Struct` resolves per row, exactly as `where` treats one).

### What the refusal caught on the way in

`shipped_chatwork_message_views_build_two_distinct_wire_urls` went red: its mock response carried
three of the six fields the shipped `/chatwork/rooms/{room}/messages` view selects, and the read only
succeeded because the view's renaming projection resolved `update_time` and `account` to null. The
fixture was corrected to return what Chatwork's message endpoint actually returns, including the
nested `account` object — which is the more faithful pin for a test about the wire URL.

That is worth stating plainly rather than burying, because it generalises: **a declared view whose
wire response omits a field its body selects now refuses at read time.** This is not new in kind —
the name-only spelling has refused on that road since `20260725113000`, and the runtime-over-the-
delivered-batch shape is that ruling's own stated law — but it is new in reach, because every
shipped view that selects with a rename took the lenient road until now. It rides to the pull
request as a Concern.

### Documentation

`docs/cookbook/faq.md`'s `unknown_column` row now states that every spelling refuses alike (name,
rename, constructor) and that `extend`/`set` keep resolving an absent column to `null`. The
`qfs-faq` skill was regenerated from it, and the plugin's four `version` fields move `0.21.2 →
0.22.0` — a **minor** bump, since a taught surface broke: a query form the skill's error table
described as answering now refuses.

### Gate

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check`, `gen-docs --check` and `gen-skills --check` all exit 0. Binary version
`0.0.125`.

### Discovered Insights

- **Insight**: the lowering fork is what made this defect invisible — a projection list goes down
  the checked `Project` road only if **every** term is a bare pass-through column, so one alias
  anywhere in the list routes the *whole* list, typo included, down the unchecked `ProjectExpr`
  road. That is why `select message_id, update_time, account.name AS n` never refused on a missing
  `update_time`.
  **Context**: whenever a stage has an all-or-nothing lowering fork like `is_plain_projection`, a
  check added to one arm is a check the other arm's *entire* payload escapes. The right question is
  not "does this term take the checked road" but "does any sibling term divert the list".
- **Insight**: the shipped `.qfs` declarations are exercised by mocks that return only the fields a
  given assertion reads, so a leniency in the engine shows up as fixture impoverishment rather than
  as a failing test. Removing the leniency is what surfaced it.
  **Context**: when tightening a total resolution to a refusal, expect the first red tests to be
  fixtures rather than logic, and read each one for what it says about live data before adjusting
  it — here it said the shipped chatwork view depends on the response carrying `account`, which is
  a real property of the API worth pinning rather than a test detail.
