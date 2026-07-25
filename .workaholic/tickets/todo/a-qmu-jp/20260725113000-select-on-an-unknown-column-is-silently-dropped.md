---
created_at: 2026-07-25T11:30:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: a-where-predicate-is-honored-or-refused-never-dropped
---

# `select` on an unknown column is silently dropped — the last stage that can mean nothing

## Overview

`|> select <col>` naming a column the relation does not carry returns **rows at exit 0** with that
column simply missing from the output. A projection of only unknown names returns the row count
with an **empty schema**; a mixed projection silently drops the unknown half. Unlike `where` and
`expand` — both of which now refuse (this mission, 2026-07-25) — projection still lets a stage mean
nothing while the query answers.

This is the item the sibling ticket `20260717180100` deliberately left open. That ticket's mechanism
section cited projection as the place where an unknown column *is* a hard error, quoting
`typeck.rs:138` (*"projection is where an unknown column is a hard error (t05)"*). Measured, it is
not — and the comment has since been corrected to say so. What remains is the decision the comment
was standing in for: **should projection refuse too?**

## Measured (2026-07-25, `qfs 0.0.90`, branch `work-20260724-011029`)

Fixture: two `.md` files in a scratch directory. Raw `echo "EXIT=$?"` after each run.

```
$ qfs run "/local<FIX> |> select nosuchcol"
{"schema":[],"rows":[{},{}],"meta":{"row_count":2,...}}
EXIT=0

$ qfs run "/local<FIX> |> select name, nosuchcol"
{"schema":[{"name":"name","type":"text"}],"rows":[{"name":"a.md"},{"name":"b.md"}],"meta":{"row_count":2,...}}
EXIT=0
```

The first is the sharper of the two: **two rows of nothing**, at success. A consumer branching on
`row_count` reads "2 results"; a consumer reading the rows gets empty objects.

**The two `select` paths disagree, and one of them already refuses.** `/sql` rejects the same query
with a structured error, because the SQL compiler validates the projection against the table catalog
(`sql-core/src/compile.rs`, the `(projection)` arm) — the same validation that was missing for
`WHERE` until this mission added it. So the behaviour is already inconsistent **between drivers**,
which is the strongest argument that the engine's projection is the odd one out.

## Mechanism

- `packages/qfs/crates/engine/src/eval.rs` — `project()` states it plainly in its own doc:
  *"Project a batch to a column list (`*`/empty is identity). **Unknown columns are dropped.**"* It
  builds `indices` with a `filter_map` over `schema.columns.iter().position(...)`, so a name that
  resolves to nothing contributes no column and no error. Its return type is `RowBatch`, not
  `Result` — the same "no channel to report" shape `expand` had before this mission.
- `packages/qfs/crates/pushdown/src/lower.rs` (`PipeOp::Select`) lowers to `LogicalPlan::Project`
  without consulting the schema; `planner.rs`'s `project_schema` likewise `filter_map`s.
- `packages/qfs/crates/core/src/eval.rs` — `project_schema` DOES refuse via `Schema::project`, but
  that fold is only reached by statements carrying `transform`/`of`/`call`/`switch`, not by a plain
  read. (It was made lenient for an EMPTY input schema by ticket `20260717180300`; that leniency is
  the undescribable case and should stay.)

## Scope

**In scope:** deciding, and then implementing, whether `|> select <col>` naming a column absent from
a **non-empty, described** relation is a structured `unknown_column` error at a non-zero exit — on
the path a real `qfs run` read takes — or stays a documented silent drop.

**The decision is the deliverable.** Both answers are defensible and this ticket must not assume the
refusal:

- **Refuse** — consistent with `where`/`expand`/`/sql`, and "two rows of nothing at exit 0" is the
  same wrong-answer-at-exit-0 class this mission exists to remove.
- **Keep the drop** — a projection over a heterogeneous or late-bound relation (a union of sources,
  a declared driver whose rows vary) may legitimately name a column only some rows carry, and
  dropping is the forgiving behaviour there. If this is chosen, `eval.rs`'s doc and the language
  reference must say so where an operator will read it, and the asymmetry with `/sql` must be
  stated rather than left to be discovered.

**Out of scope:**

- `where` and `expand` — both refuse as of this mission; do not revisit.
- The empty-schema (undescribable) leniency — settled: late-bound relations are never refused.
- Teaching the planner a decoded relation's schema (ticket `20260717180300`'s deep change).

## Key Files

- `packages/qfs/crates/engine/src/eval.rs` — `project()` and its "unknown columns are dropped" doc;
  the `filter_checked`/`expand` pattern next to it is the shape a refusal would follow.
- `packages/qfs/crates/engine/src/combine.rs` — the `CombineOp::Project` arm and
  `EngineError::UnknownColumn` (already exists, already carries `stage`, so a `"select"` stage costs
  nothing new).
- `packages/qfs/crates/pushdown/src/planner.rs` — `project_schema`'s `filter_map`.
- `packages/qfs/crates/core/src/eval.rs` — `project_schema`, the fold that already refuses.
- `packages/qfs/crates/sql-core/src/compile.rs` — the driver that already refuses, for comparison.

## Policies

- `workaholic:design` — 「推測するな、宣言して拒否せよ」. A `select` naming an undeclared column is a
  malformed question; answering it with rows of nothing is a guess about what the caller meant.
- `workaholic:implementation` / `objective-documentation` — whichever way this lands, the doc and the
  binary must agree, and the `/sql`-vs-engine asymmetry must be recorded.
- `workaholic:development` / `qa-engineering` — verified by a both-directions test.

## Quality Gate

Verify with **raw exit codes** — `echo "EXIT=$?"` immediately after each command; never pipe a gate
through `tail`.

1. **The decision is recorded** in the ticket outcome and the commit body, with its reason — not
   inferred from the diff.
2. If refusal is chosen: both runs above return a structured `unknown_column` naming the column and
   the available list, at a non-zero exit, with the actual output pasted; and a both-directions test
   (red before, green after) covers the only-unknown and the mixed projection.
3. **`*` and an empty projection stay identity**, pinned by a test.
4. **A late-bound / undescribable relation is not refused** — the same leniency `where` and `expand`
   keep. Pinned by a test.
5. **`/sql` is unchanged** — it already refuses; whichever way the engine goes, its behaviour must be
   stated as consistent-or-deliberately-different, not silently diverged further.
6. **Workspace gates green, raw exit codes shown**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --workspace`, plus `gen-docs --check` /
   `gen-skills --check` if a taught surface moves.

## Considerations

- Minted by the `/monitor` drive of this mission (run 20260725-101714) after fixing its four
  tickets. It is **not** a regression from that work: the behaviour is unchanged, but the mission's
  own Experience section ("a query stage that cannot be honored must never be silently ignored")
  now covers every stage except this one, so leaving it unrecorded would misrepresent the mission as
  complete in a way it is not.
- The engine's `EngineError::UnknownColumn` already carries a `stage` field, and `project` sits
  directly beside the `filter_checked`/`expand` refusals it would copy — the implementation is
  small. The cost of this ticket is the judgement, not the code.
