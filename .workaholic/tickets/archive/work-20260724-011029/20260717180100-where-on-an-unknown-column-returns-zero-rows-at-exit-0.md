---
created_at: 2026-07-17T18:01:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 4h
commit_hash:
category: Changed
depends_on: [20260723020055-gdrive-where-pushdown-silent-drop.md]
mission: a-where-predicate-is-honored-or-refused-never-dropped
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

## Final Report — the refusal lives at the seam that sees the delivered rows (2026-07-25)

**Where the check had to go, and why not where the ticket looked.** The mechanism section is
accurate: `typeck.rs`'s `column_type` collapses "absent column" into `Unknown`, and
`engine/src/eval.rs`'s `resolve → None → false` drops the row. But neither is the place to fix it,
because **neither layer knows the relation's columns**. The plan-time fold (`core/src/eval.rs`) is
not on a plain read's path, and the lowering the read DOES take (`pushdown/src/lower.rs`) gets its
schema from `plan.rs`'s `schema_of`, which describes the **driver ROOT** (`describe_root`) — for
`/sql/db/users` that is the `/sql` catalog, not the table. A check there would have false-rejected
every real column of every addressed node.

The one schema that is certainly the relation's is the one the **driver actually delivered**. So the
refusal runs in `qfs-engine`, over the batch:

- `eval::filter_checked` — the caller-written `WHERE`. Used by `CombineOp::Filter` (every driver
  that pushes no predicate: `/local`, `/sys`, `/git`, `/type`, `/collections`, declared drivers …)
  and by the read executor's pushed-filter seam (`apply_where`, for facets that do not enforce the
  predicate themselves — `/github`, `/slack`).
- `eval::filter` — unchanged and infallible — stays the **driver-residual** form. That split is
  load-bearing: a driver's truthful residual may name a backend search pseudo-column no row carries
  (Drive's `fullText`, Gmail's `label`), and refusing it would break a real capability.

**The five facets that resolve a `WHERE` inside a backend query language** never reach either, so
each got the same refusal where its own catalog lives — and in three of them the inconsistency was
already glaring, since a projected or ordered unknown name was ALREADY a structured error while a
filtered one was not:

| surface | before | now |
| --- | --- | --- |
| `/sql`, `/cf` D1 | `WHERE nosuchcol` fell through `lower_cmp`'s catalog test into the residual → `rows: []` | `SqlError::UnknownColumn { reason: "not a column of the table (WHERE)" }` |
| `/ga` | same, against the property catalog | `GaError::UnknownField { reason: "… (WHERE)" }` (`date` stays the report's own window coordinate) |
| `/mail`, `/drive` | same, against the `q=`/`q` mapping | the facet calls `qfs_exec::check_where_columns(schema, p, SEARCH_COLUMNS)`; each driver now publishes the pseudo-columns it filters on that no row carries |

### Quality Gate

1. **The defect is refused, with raw exit codes.** (`/markdown` no longer exists in this tree — the
   compiled driver was retired in favour of `/collections`, which needs a registered view; `/sys`
   contributes two independent nodes and `/type` a third relation instead.)

   ```
   $ qfs run "/local<FIX> |> where nosuchcol == 'zzz'"
   {"error":{"code":"unknown_column","kind":"usage","message":"`where` names column 'nosuchcol',
    which this relation does not carry; available: [name, path, size, modified, is_dir, mode, content]"}}
   EXIT=2

   $ qfs run "/sys/drivers |> where nosuchcol == 'zzz'"
   … available: [kind, name, base_url, auth, pagination, of_type, verb, body, irreversible, created_at]
   EXIT=2

   $ qfs run "/sys/connections |> where nosuchcol == 'zzz'"
   … available: [driver, connection, created_at]      EXIT=2

   $ qfs run "/type |> where nosuchcol == 'zzz'"
   … available: [name, columns, refinement, created_at]   EXIT=2
   ```

2. **Both directions.** `where_on_a_column_absent_from_a_described_schema_is_refused` asserts the
   code, the stage, the column and the exact available list; it cannot pass on the old code, which
   returned `Ok` with zero rows. The `/sql` and `/ga` compiler tests likewise assert an `Err` where
   the old code produced a residual (and each pins the control that still compiles).
3. **"No matches" is untouched.**
   `where_on_a_real_column_with_no_match_stays_an_empty_relation_at_success` pins it, and by run:
   `/local<FIX> |> where name == 'nonexistent.md'` → `row_count: 0`, **EXIT=0**, full schema
   preserved. `/type |> where name == 'no-such-type'` → same.
4. **The late-bound case is preserved.** `where_over_an_undescribable_schema_stays_late_bound`: an
   EMPTY schema is never refused — the predicate still executes. This is the branch
   `typeck.rs:487-488` protects, and the check's first line is `if schema.columns.is_empty()`.
   `a_dotted_path_only_requires_its_head_column` pins the other half: `meta.title` needs only
   `meta`, so Json navigation stays late-bound, while an absent HEAD is still refused.
5. **The stated rationale is reconciled — the cited guarantee does not exist.** Measured now:

   ```
   $ qfs run "/local<FIX> |> select nosuchcol"        → {"schema":[],"rows":[{},{}]}   EXIT=0
   $ qfs run "/local<FIX> |> select name, nosuchcol"  → only `name`; the unknown name is dropped   EXIT=0
   ```

   So `typeck.rs:138`'s *"projection is where an unknown column is a hard error (t05)"* is false of
   the binary on the executed read path. The comment is **corrected**, not left standing: it now
   records what was measured, names the engine's `where` check as the refusal that actually fires,
   and marks "should projection refuse too?" as an open, separate decision rather than an assumed
   one. (`/sql` is the exception that proves it — there `SELECT nosuchcol` IS refused, by the SQL
   compiler's own catalog validation, which is exactly the validation `WHERE` was missing.)
6. **Every operator, not just `Cmp`.** `every_predicate_operator_refuses_an_unknown_column_not_just_cmp`
   covers `In`, `Between`, `Like`, a `NOT` arm and an `OR` arm — all take the same
   `resolve → None → false` path. Confirmed by run on `/sys/drivers`: all five return
   `unknown_column` at **EXIT=2**. The collector walks the whole boolean structure, so a typo buried
   in a disjunct cannot hide.
7. **Workspace gates green** — `cargo test --workspace` with a raw exit code (no pipe), plus the
   branch gate. Patch bumped on this branch (0.0.89 → 0.0.90).

**Scope kept.** No projection behaviour was changed; `Json` navigation semantics are untouched; the
two plan paths were not restructured; and the late-binding posture for genuinely undescribable
schemas is intact — the fix removes only the **conflation** of "undescribable" with "described, and
this is not one of its columns".
