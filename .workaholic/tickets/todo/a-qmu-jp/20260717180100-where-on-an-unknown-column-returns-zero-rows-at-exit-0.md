---
created_at: 2026-07-17T18:01:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# `where` on an unknown column returns 0 rows at exit 0 instead of `unknown_column`

## Overview

A `where` predicate naming a column that does not exist in the source's schema returns an **empty
relation with exit 0**. The same query shape with a real column and no matches returns the same
thing: empty relation, exit 0. **A typo'd column name and "nothing matched" are indistinguishable
in both the output and the exit code.**

This is general across drivers, not driver-specific: reproduced below on `/local`, `/markdown`, and
`/sys`. Every consumer that reads a qfs result — a script branching on `row_count`, an agent
following the describe→preview→commit loop, qfs-viewer — receives "none" as the answer to a
malformed question.

Found by execution while measuring `decode md`
(`.workaholic/tickets/archive/work-20260717-160001/20260717141500-measure-whether-decode-md-can-replace-the-markdown-driver.md`,
recorded there as a finding and deliberately not fixed). Re-verified independently in this session.

## Reproduced (2026-07-17, binary `qfs 0.0.78`, branch `work-20260717-160001`)

Fixture: four `.md` files in a scratch directory; a scratch `path_binding` `/markdown/t2verify`
bound to it and removed afterwards (`/sys/connections` verified back to its baseline 10 rows).
Every run carries a raw `echo "EXIT=$?"`; no pipes mask any exit code.

### The defect — three drivers, same result

```
$ qfs run "/local<FIX> |> where nosuchcol == 'zzz'"
{"schema":[{"name":"name",...},{"name":"content","type":"bytes"}],"rows":[],
 "meta":{"row_count":0,...}}
EXIT=0

$ qfs run "/markdown/t2verify/documents |> where nosuchcol == 'x'"
{"schema":[{"name":"path",...},{"name":"frontmatter","type":"json"}],"rows":[],
 "meta":{"row_count":0,...}}
EXIT=0

$ qfs run "/sys/drivers |> where nosuchcol == 'zzz'"
{"schema":[{"name":"kind",...},{"name":"created_at","type":"text"}],"rows":[],
 "meta":{"row_count":0,...}}
EXIT=0
```

`/sys/drivers` returns **30 rows** unfiltered and `/markdown/t2verify/documents` returns **4**; both
return 0 rows through a predicate on a column that does not exist, at exit 0.

### The controls that establish indistinguishability

```
$ qfs run "/local<FIX> |> where name == 'a.md'"            # known column, 1 match
  → row_count: 1    EXIT=0
$ qfs run "/local<FIX> |> where name == 'nonexistent.md'"  # known column, 0 matches
  → row_count: 0    EXIT=0
$ qfs run "/local<FIX> |> where nosuchcol == 'zzz'"        # UNKNOWN column
  → row_count: 0    EXIT=0
```

The last two runs are byte-identical in `rows`, `meta`, schema, and exit code. Nothing in the
response distinguishes them.

## Mechanism (verified in the tree at `91cde7d`)

Two independently-documented "conservative" decisions compose into the silent answer. Neither layer
errors, and the read path never reaches the layer that would.

### 1. Plan time: an absent column is typed as late-bound `Unknown`, which passes the check

- `packages/qfs/crates/core/src/typeck.rs:486-493` — `column_type(name, schema)` returns
  `ColumnType::Unknown` when the schema is empty **and** when the column is absent:
  `schema.column(name).map_or(ColumnType::Unknown, |c| c.ty.clone())`. Its doc (`:483-485`) states
  the intent: *"late-binding (`Unknown`) when the schema is itself late-bound (empty /
  undescribable) **or the column is absent** — the conservative posture that never false-rejects a
  column from a driver that does not (yet) describe it."*
- `typeck.rs:137-139` records the same decision at the expression level: *"An unresolved column
  stays late-bound (`Unknown`) rather than erroring here — projection is where an unknown column is
  a hard error (t05); a `WHERE` over an undescribable column degrades to late-bound, preserving the
  pre-t75 leniency."*
- `typeck.rs:272-275` — once either operand is `Unknown` or `Json`, the comparison check returns
  `Ok(Ty::unknown())` without further checking.

**The two cases the doc names are collapsed into one answer.** `column_type` returns `Unknown` both
for "this driver does not describe its columns" (where the leniency is load-bearing) and for "this
driver described its columns and this is not one of them" (where nothing is being protected).
`/local`, `/sys`, and `/markdown` all describe their columns fully — every schema in the runs above
is non-empty and complete.

### 2. Run time: an unresolvable column makes the predicate false, dropping the row

- `packages/qfs/crates/engine/src/eval.rs:48-59` — `resolve()` returns `None` when the column is
  not in the schema (`schema.columns.iter().position(|c| &c.name == head)?`). Its doc: *"Missing/
  unnavigable ⇒ `None`."*
- `engine/src/eval.rs:27-30` — `Predicate::Cmp(col, op, lit) => match resolve(col, schema, row) {
  Some(v) => cmp(&v, *op, lit), None => false }`. An unresolvable column yields `false`; the row is
  dropped. `In`/`Between`/`Like` (`:31-42`) do the same.
- `engine/src/eval.rs:19-20` documents it: *"Total: a comparison whose operands are not comparable
  evaluates to `false` (the row does not match)."*
- `engine/src/eval.rs:125-133` — `filter()` returns a `RowBatch`, not a `Result`; it has no channel
  to report a column error. Call site: `engine/src/combine.rs:210`.

`resolve()` returns `None` for two different situations — *the column is absent from the schema* and
*the value is null / the path is unnavigable* — and the predicate maps both to "row does not match".

### 3. The typed path that DOES validate is not on the read path

`packages/qfs/crates/core/src/eval.rs:796-801` type-checks the predicate:

```rust
PipeOp::Where(predicate) => {
    self.typecheck_predicate(predicate, input.schema())?;
    Ok(PlanSource::Filter { input: Box::new(input) })
}
```

Its comment (`core/src/eval.rs:790-795`) claims the guarantee this ticket reports as violated:
*"The filter predicate is **type-checked at plan time** against the input schema (decision T, ticket
t75) … is a structured plan-time error here — before any I/O, so a type-failing pipeline never
reaches preview/commit."*

Two reasons it does not fire for an unknown column:

1. Even when reached, `typecheck_predicate` (`core/src/eval.rs:513-518`) delegates to
   `typeck::check_expr`, which types the absent column as `Unknown` and passes it (§1 above). It is
   also gated on `if let Some(stdlib) = self.stdlib` — with no stdlib wired it checks nothing at all.
2. The executed read path lowers through `packages/qfs/crates/pushdown/src/lower.rs:250`
   (`PipeOp::Where(e) => Ok(LogicalPlan::Filter { … })`), which does not consult the schema.

### The safety net the leniency defers to does not exist

`typeck.rs:138` justifies the `where` leniency by pointing at projection: *"projection is where an
unknown column is a hard error (t05)"*. Projection does not hard-error on the executed read path
either:

```
$ qfs run "/markdown/t2verify/documents |> select nosuchcol"
{"schema":[],"rows":[{},{},{},{}],"meta":{"row_count":4,...}}
EXIT=0

$ qfs run "/markdown/t2verify/documents |> select title, nosuchcol"
{"schema":[{"name":"title","type":"text"}],"rows":[{"title":"Alpha"},...],"meta":{"row_count":4,...}}
EXIT=0
```

`engine/src/eval.rs:135` states the behavior: *"Project a batch to a column list (`*`/empty is
identity). **Unknown columns are dropped.**"* A projection naming only unknown columns yields an
empty schema with the row count preserved; a mixed projection silently drops the unknown name.

This is recorded as **evidence about the `where` decision's stated rationale**, not as a second
ticket: the cited hard error is the reason `where` was made lenient, and it does not fire.

## Scope

**In scope:** a `where` predicate naming a column absent from a **non-empty, described** schema
resolves to a structured `unknown_column` error rather than an empty relation at exit 0 — on the
path a real query takes.

**Out of scope / do not do in passing:**

- **Removing the late-binding posture for genuinely undescribable schemas.** `typeck.rs:483-485`
  names a real case: a driver that does not describe its columns. Where `schema.columns.is_empty()`
  (`typeck.rs:487-488`), late-binding must stay. The defect is the **conflation** of that case with
  a described schema missing the column, not the leniency itself.
- **Changing `Json` navigation semantics.** A dotted path into a `Json`/`Struct` column
  (`typeck.rs:150-154`, `engine/src/eval.rs:52-57`) is late-bound by design; this ticket is about a
  **bare head column** absent from the schema.
- **Deciding projection's behavior.** The `select` measurements above are recorded as evidence
  about the rationale. Whether projection should hard-error is a separate decision; do not settle
  it while fixing `where`.
- **Reconciling the two plan paths.** That `core/src/eval.rs`'s typed fold is not what a read
  executes (`pushdown/src/lower.rs`) is a structural fact this ticket reports and works within. Do
  not restructure the planner here.

## Key Files

- `packages/qfs/crates/core/src/typeck.rs:483-493` — `column_type`, where the absent-column and
  empty-schema cases are collapsed to `Unknown`.
- `packages/qfs/crates/core/src/typeck.rs:133-149,272-275` — expression-level late-binding and the
  `Unknown`/`Json` short-circuit.
- `packages/qfs/crates/core/src/eval.rs:790-801,513-518` — the plan-time typecheck and its stdlib
  gate; the comment asserting the guarantee.
- `packages/qfs/crates/pushdown/src/lower.rs:250` — the lowering the read path actually takes.
- `packages/qfs/crates/engine/src/eval.rs:19-44,48-59,125-133,135` — `eval_predicate`, `resolve`,
  `filter`, `project`.
- `packages/qfs/crates/engine/src/combine.rs:210` — the infallible filter call site.

## Policies

- `workaholic:design` — 「推測するな、宣言して拒否せよ」 ("declare, don't guess; refuse the
  undeclared"). A query naming an undeclared column must be refused, not answered with a relation
  that reads as a fact about the data.
- `workaholic:implementation` — a total function that maps two distinct conditions ("absent column",
  "no match") onto one indistinguishable output removes the caller's ability to tell them apart;
  the gap should be machine-checkable at plan time, before I/O.
- `workaholic:safety` — a wrong answer delivered at exit 0 is consumed as a right one. `/sys` reads
  are administrative surfaces where an empty result is read as an assertion about the system.
- `workaholic:development` / `qa-engineering` — the fix is verified by a both-directions test (the
  new behavior passes, the current behavior fails it), not by review.

## Quality Gate

Verify with **raw exit codes** — `echo "EXIT=$?"` immediately after each command. Never `cmd | tail`
or `|| true`; both mask the status this ticket is about.

1. **The defect is refused, on all three drivers.** `|> where nosuchcol == 'x'` over `/local`,
   `/markdown/<tree>/documents`, and `/sys/drivers` returns a structured `unknown_column` error
   naming the offending column and the available columns, at a non-zero exit. Actual command,
   output, and exit code pasted for each.
2. **Both directions.** A test that fails on the current code and passes after the change — for at
   least the `where` case on a described schema. A test that only passes after is not sufficient
   evidence the behavior moved.
3. **"No matches" is untouched.** `|> where name == 'nonexistent.md'` still returns an empty
   relation at exit 0. An empty result must remain a valid, non-error answer for a real column —
   pinned by a test.
4. **The late-bound case is preserved.** A predicate over a source whose schema is empty /
   undescribable still passes plan time and executes (`typeck.rs:487-488`'s branch). Demonstrated by
   a test that would fail if the fix rejected an undescribable driver's column.
5. **The stated rationale is reconciled.** `typeck.rs:138` cites projection as the hard-error site
   for an unknown column, and the runs above show projection dropping unknown columns silently at
   exit 0. Either correct the comment to match the shipped behavior, or record explicitly why it
   stands. Do not leave a comment that justifies this decision by a guarantee the binary does not
   provide.
6. **Every operator that resolves a column is checked, not just `Cmp`.** `In`, `Between`, and `Like`
   (`engine/src/eval.rs:31-42`) take the same `resolve → None → false` path. State, by run or test,
   what each does with an unknown column after the change.
7. **Workspace gates green, raw exit codes shown**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo run -p xtask --
   gen-docs --check` if any describable surface moved. Patch version bumped per CLAUDE.md when this
   reaches a PR.

## Considerations

- **Severity relative to the two sibling tickets filed alongside this one** (`20260717180200`
  `expand` no-op, `20260717180300` codec error names the wrong columns): this one is the only defect
  of the three that returns a **plausible answer**. The other two either no-op visibly or produce an
  error naming wrong columns; both leave the operator with something to notice. A query language that
  answers "none" to a malformed question corrupts every consumer that trusts the answer, silently.
- The `expand` ticket (`20260717180200`) shares the shape — a validating typed path that the read
  path does not reach — and both may resolve against the same seam. They are filed separately
  because the mechanisms differ (`expand` swallows an error that IS raised; `where` never raises
  one) and either can land alone.
- The two-path structure (`core/src/eval.rs` typed fold vs. `pushdown/src/lower.rs` → `engine`) is
  the reason a plan-time claim in a comment is not evidence about runtime behavior. Whatever fix
  lands, its test must exercise the path a real `qfs run` takes.
