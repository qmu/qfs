---
created_at: 2026-07-17T18:02:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# `expand` silently no-ops at exit 0 on a Json column and on a column that does not exist

## Overview

`|> expand <column>` returns the input relation **unchanged, at exit 0**, in two cases:

1. the column is a `Json` column, and
2. the column does not exist at all.

`Schema::expand` (`packages/qfs/crates/types/src/schema.rs:322`) documents both as errors —
`NotExpandable` and `UnknownColumn` — and its doc states plainly (`:316-317`) that *"Expanding a
scalar / `Json` / `Unknown` column is rejected."* On the executed read path neither error reaches a
caller: one is discarded by `unwrap_or`, the other short-circuits before the check.

`expand` **does** work on a real `Array` column (3 rows → 4 rows, verified below), so the stage is
not inert — it succeeds, no-ops, and errors are indistinguishable from each other at the CLI.

**Consequence worth stating: "expand the frontmatter" has no operator on either path.** The
`/markdown` driver carries `frontmatter` as a `Json` column, where `expand` silently no-ops; the
`decode md` codec splats frontmatter keys to top-level columns itself but is single-blob only and
cannot be followed by a relational stage. This lands squarely on a route proposed in design
discussion (2026-07-17): 「集めてきたマークダウンファイルのフロントマターなどメタ情報を decode して
**展開する**ところまでは qfs-query で表現できるようにしたい」. The 「展開する」 half has no working
operator today.

Found by execution while measuring `decode md`
(`.workaholic/tickets/archive/work-20260717-160001/20260717141500-measure-whether-decode-md-can-replace-the-markdown-driver.md`,
recorded there as a finding and deliberately not fixed). Re-verified independently in this session.

## Reproduced (2026-07-17, binary `qfs 0.0.78`, branch `work-20260717-160001`)

Fixture: four `.md` files with YAML frontmatter, ATX headings, and cross-file markdown links, in a
scratch directory. A scratch `path_binding` `/markdown/t2verify` bound to it and removed afterwards
(`/sys/connections` verified back to its baseline 10 rows). Raw `echo "EXIT=$?"` throughout.

### Positive control — `expand` works on a real Array column

```
$ qfs run "/markdown/t2verify/links"
  → row_count: 3, source_section_path: "type":"array"
EXIT=0

$ qfs run "/markdown/t2verify/links |> expand source_section_path"
{"schema":[{"name":"source_doc","type":"text"},{"name":"source_section_path","type":"text"},...],
 "rows":[{"source_doc":"alpha.md","source_section_path":"Alpha Heading",...},
         {"source_doc":"alpha.md","source_section_path":"Alpha Heading",...},
         {"source_doc":"alpha.md","source_section_path":"Alpha Subsection",...},
         {"source_doc":"beta.md","source_section_path":"Beta Heading",...}],
 "meta":{"row_count":4,...}}
EXIT=0
```

**3 rows → 4 rows**, and the column type moves `array` → `text`. The stage executes.

### Defect 1 — a Json column: no-op, no `NotExpandable`

```
$ qfs run "/markdown/t2verify/documents"
  → row_count: 4, frontmatter: "type":"json"
EXIT=0

$ qfs run "/markdown/t2verify/documents |> expand frontmatter"
{"schema":[{"name":"path","type":"text"},{"name":"title","type":"text"},
           {"name":"frontmatter","type":"json"}],
 "rows":[{"path":"a.md","title":"Alpha","frontmatter":{"tags":["x","y"],"title":"Alpha"}},...],
 "meta":{"row_count":4,...}}
EXIT=0
```

Schema **unchanged** (`frontmatter` still `json`), rows **unchanged**, exit 0. No `NotExpandable`.

### Defect 2 — a column that does not exist: no-op, no `UnknownColumn`

```
$ qfs run "/markdown/t2verify/documents |> expand nosuchcol"
{"schema":[{"name":"path","type":"text"},{"name":"title","type":"text"},
           {"name":"frontmatter","type":"json"}],
 "rows":[{"path":"a.md","title":"Alpha",...},...],
 "meta":{"row_count":4,...}}
EXIT=0
```

Byte-identical to the unfiltered `documents` read. No `UnknownColumn`.

### The validation fires only when a downstream stage forces schema resolution at lowering

```
$ qfs run "/local<FIX> |> expand nosuchcol"
  → EXIT=0, silently unchanged

$ qfs run "/local<FIX> |> expand nosuchcol |> transform triage"
{"error":{"code":"unknown_column","kind":"usage","message":"Type(UnknownColumn { name: \"nosuchcol\",
 available: [\"name\",\"path\",\"size\",\"modified\",\"is_dir\",\"mode\",\"content\"] })"}}
EXIT=2
```

The same pipeline reports the error the pure read swallows — only because `transform` forces
resolution. The check exists; the read path does not reach it.

## Mechanism (verified in the tree at `91cde7d`)

The executed read path is `pushdown/src/lower.rs:326-329` → `engine/src/combine.rs:227` →
`engine/src/eval.rs:480`. Every error channel is closed along it.

### `engine/src/eval.rs:480` — the executed implementation cannot report an error

```rust
480  pub(crate) fn expand(batch: RowBatch, field: &Name) -> RowBatch {
481      let Some(idx) = batch.schema.columns.iter().position(|c| &c.name == field) else {
482          return batch;
483      };
484      // Output schema: replace the field column per the type model's `expand`.
485      let schema = batch.schema.expand(field).unwrap_or(batch.schema.clone());
```

Its return type is `RowBatch`, **not** `Result<RowBatch, _>` — it has no channel to report either
documented error. Three swallow points:

- **`:481-483`** — an absent column returns the batch **unchanged**. This is `UnknownColumn`,
  discarded before `Schema::expand` is ever called.
- **`:485`** — `.unwrap_or(batch.schema.clone())` **discards** `Schema::expand`'s `Err`, keeping the
  original schema. This is where `NotExpandable` for a `Json` column is dropped.
- **`:498-499`** — `// A scalar/Null field is not expandable: keep the row unchanged.` →
  `other => out_rows.push(splice_row(&row, idx, vec![other]))`. A `Json` value falls to this arm and
  the row passes through, so the rows agree with the un-updated schema and nothing looks wrong
  downstream.

Call site `engine/src/combine.rs:227`:
`CombineOp::Expand(field) => Ok(eval::expand(unary(inputs, cursor, transform)?, field))` — wrapped
in `Ok`, infallible by construction.

### `types/src/schema.rs:315-322` — the contract the runtime does not keep

```
315      ///
316      /// Other columns are preserved in place. Expanding a scalar / `Json` / `Unknown`
317      /// column is rejected.
318      ///
319      /// # Errors
320      /// - [`TypeError::UnknownColumn`] if `field` is absent.
321      /// - [`TypeError::NotExpandable`] if `field` is not an `Array`/`Struct`.
322      pub fn expand(&self, field: &Name) -> Result<Schema, TypeError> {
```

`NotExpandable` is returned at `schema.rs:345`. The function is correct; its only executed caller
throws the result away.

### The typed path that DOES propagate is not on the read path

`packages/qfs/crates/core/src/eval.rs:839-846`:

```rust
PipeOp::Expand(field) => {
    let name = field.last().cloned().unwrap_or_default();
    let schema = input.schema().expand(&name)?;   // <- propagates
    Ok(PlanSource::Expand { input: Box::new(input), schema })
}
```

This fold **does** propagate both errors with `?`. It is not what a `qfs run` read executes — the
read lowers through `pushdown/src/lower.rs:326-329`, which builds `LogicalPlan::Expand` from the
field name **without consulting the schema at all**:

```rust
PipeOp::Expand(field) => Ok(LogicalPlan::Expand {
    input: Box::new(input),
    field: field.last().cloned().unwrap_or_default(),
}),
```

This is the same structural shape as ticket `20260717180100` (`where` on an unknown column): a
validating typed path that the executed read path does not reach.

## Scope

**In scope:** on the path a real `qfs run` read takes, `|> expand <col>` surfaces
`Schema::expand`'s documented errors — `UnknownColumn` for an absent column, `NotExpandable` for a
`Json`/scalar column — as structured errors at a non-zero exit, instead of returning the input
unchanged at exit 0.

**Out of scope / do not decide in passing:**

- **Whether `expand` SHOULD work on `Json`.** This ticket is about the gap between the documented
  contract and the runtime, not about widening the contract. `schema.rs:316-317` says a `Json`
  column *is rejected*; making the rejection real is this ticket. **Implementing `Json` expansion
  is a different change** — it is item 5 of the `decode md` measurement's "what would have to
  change" list and belongs to that decision, which is the developer's. If `Json` expansion is what
  is wanted, this ticket delivers the honest error first and that work is scoped separately.
- **The `decode md` / `/markdown` driver question.** The archived measurement is evidence for a
  developer decision, not a mandate. The `/markdown` driver ships, its mission is 7/7, and
  qfs-viewer PR #11 consumes `/markdown/<name>/documents|links`. Nothing here removes, deprecates,
  or changes the driver.
- **`Array`/`Struct` expansion semantics.** The positive control shows both work; leave them.
- **Reconciling the two plan paths** (`core/src/eval.rs` fold vs. `pushdown/src/lower.rs`). A
  structural fact this ticket works within, not a restructure to attempt here.

## Key Files

- `packages/qfs/crates/engine/src/eval.rs:476-503` — the executed `expand`; the three swallow
  points at `:481-483`, `:485`, `:498-499`.
- `packages/qfs/crates/engine/src/combine.rs:227` — the infallible call site.
- `packages/qfs/crates/types/src/schema.rs:315-322,345` — the documented contract and
  `NotExpandable`.
- `packages/qfs/crates/pushdown/src/lower.rs:326-329` — the read-path lowering that skips the
  schema.
- `packages/qfs/crates/core/src/eval.rs:839-846` — the typed fold that propagates, and is not
  reached on a read.
- `packages/qfs/crates/driver-markdown/src/schema.rs` — `documents.frontmatter` as the shipped
  `Json` column this is measured against.

## Policies

- `workaholic:design` — 「推測するな、宣言して拒否せよ」. A stage handed a column it cannot expand
  must refuse, not return the input and let the caller believe it expanded.
- `workaholic:implementation` / `type-driven-design` — a documented `# Errors` contract that the
  only executed caller discards with `unwrap_or` is not enforced by the type system; the seam should
  make the error unswallowable rather than rely on review.
- `workaholic:implementation` / `objective-documentation` — `schema.rs:316-317` asserts a rejection
  the binary does not perform. Doc and behavior must agree in whichever direction is chosen.
- `workaholic:development` / `qa-engineering` — verified by a both-directions test, not by review.

## Quality Gate

Verify with **raw exit codes** — `echo "EXIT=$?"` immediately after each command. Never `cmd | tail`
or `|| true`.

1. **The Json case is refused.** `/markdown/<tree>/documents |> expand frontmatter` returns a
   structured `NotExpandable` (naming the column and its type) at a non-zero exit. Actual command,
   output, exit code pasted.
2. **The unknown-column case is refused.** `/markdown/<tree>/documents |> expand nosuchcol` returns
   a structured `UnknownColumn` naming the column and the available columns, at a non-zero exit.
   Same evidence.
3. **Both directions.** A test that fails on the current code and passes after — for each of the two
   cases. A test that only passes after does not show the behavior moved.
4. **The Array case is untouched.** `/markdown/<tree>/links |> expand source_section_path` still
   returns 4 rows from 3 at exit 0, with `source_section_path` moving `array` → `text`. Pinned by a
   test, and shown by a run.
5. **`Struct` expansion is untouched** (`engine/src/eval.rs:495-496`, `schema.rs:341-343`) — pinned
   by a test.
6. **The swallow points are closed, not worked around.** `engine/src/eval.rs:485`'s
   `.unwrap_or(...)` no longer discards an `Err`, and `:481-483` no longer returns early on an
   absent column. If the fix instead validates at lowering (`pushdown/src/lower.rs:326-329`), state
   which side owns the refusal and why, and record it in the commit body — but a `RowBatch`-returning
   `expand` that still cannot report an error must not remain the only guard.
7. **The doc and the behavior agree.** After the change, `schema.rs:316-317` ("Expanding a scalar /
   `Json` / `Unknown` column is rejected") is either true of the binary, or corrected.
8. **Workspace gates green, raw exit codes shown**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --workspace`, plus `cargo run -p xtask --
   gen-docs --check` / `gen-skills --check` if any describable or taught surface moved. Patch bumped
   per CLAUDE.md when this reaches a PR.

## Considerations

- **The `decode md` measurement's finding stands unchanged by this ticket**: `decode md` cannot
  replace the `/markdown` driver, and the ATX heading-stack / `links` producer remains the only real
  implementation target on that route. This ticket fixes an honesty gap in `expand`; it does not
  advance or retire that direction.
- Fixing this makes the developer's 「展開する」 route **fail loudly instead of silently** — it does
  not make it work. That is the intended outcome here: an honest error is the precondition for
  deciding whether `Json` expansion should be built.
- Shares the "validating typed path the read path never reaches" shape with ticket
  `20260717180100`. Filed separately because the mechanisms differ — `expand` swallows an error that
  IS raised (`unwrap_or`), while `where` never raises one (an absent column types as `Unknown`) —
  and either can land alone. If both are driven together, the two-path structure is the common seam.
