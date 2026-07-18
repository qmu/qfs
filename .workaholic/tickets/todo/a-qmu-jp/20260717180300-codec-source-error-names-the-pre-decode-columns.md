---
created_at: 2026-07-17T18:03:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# A codec source's `unknown_column` error names the pre-decode columns

## Overview

`PlanSource::Codec` reports its **input's** schema — the schema *before* the decode — so an error
raised against a pipeline containing `decode` names the **blob source's listing columns** rather
than the decoded ones. An operator asking why `front_matter` is unknown is told the available
columns are `name, path, size, modified, is_dir, mode, content`: the columns of the file listing,
not of the decoded document.

`packages/qfs/crates/core/src/eval.rs:145-158`:

```rust
145          match self {
146              PlanSource::Scan { schema, .. }
...
153              | PlanSource::Transform { schema, .. } => schema,
154              PlanSource::Filter { input }
155              | PlanSource::Shape { input }
156              | PlanSource::Codec { input, .. } => input.schema(),
157          }
```

`Filter` and `Shape` are schema-preserving, so reporting `input.schema()` is correct for them.
`Codec` is not schema-preserving: `decode md` replaces the listing's columns with the decoded
document's columns entirely (measured below). It reports a schema the relation does not have.

Because this error is raised at lowering, **it fires first and masks every later failure in the
pipeline** — the `decode md` measurement had to isolate each subsequent failure by hand after this
one hid them.

Found by execution while measuring `decode md`
(`.workaholic/tickets/archive/work-20260717-160001/20260717141500-measure-whether-decode-md-can-replace-the-markdown-driver.md`,
recorded there as a finding and deliberately not fixed). Re-verified independently in this session.

## Reproduced (2026-07-17, binary `qfs 0.0.78`, branch `work-20260717-160001`)

Fixture: four `.md` files with YAML frontmatter in a scratch directory. Raw `echo "EXIT=$?"`
throughout.

### The error names the wrong columns

```
$ qfs run "/local<FIX> |> where path like '%.md' |> decode md |> expand front_matter |> transform summarize"
{"error":{"code":"unknown_column","kind":"usage","message":"Type(UnknownColumn { name: \"front_matter\",
 available: [\"name\",\"path\",\"size\",\"modified\",\"is_dir\",\"mode\",\"content\"] })"}}
EXIT=2
```

### The same error on a SINGLE file — the row count is not the cause

```
$ qfs run "/local<FIX>/alpha.md |> decode md |> expand front_matter |> transform triage"
{"error":{"code":"unknown_column","kind":"usage","message":"Type(UnknownColumn { name: \"front_matter\",
 available: [\"name\",\"path\",\"size\",\"modified\",\"is_dir\",\"mode\",\"content\"] })"}}
EXIT=2
```

A single-file read, so no row-count guard applies. The `available` list is still the pre-decode
listing columns.

### What the relation's columns actually are at that point

```
$ qfs run "/local<FIX>/alpha.md |> decode md"
{"schema":[{"name":"author","type":"text"},{"name":"status","type":"text"},
           {"name":"tags","type":"array"},{"name":"title","type":"text"},
           {"name":"body","type":"text"}],
 "rows":[{"author":"a@qmu.jp","status":"active","tags":["one","two"],
          "title":"Alpha Document","body":"# Alpha Heading\n\n..."}],
 "meta":{"row_count":1,...}}
EXIT=0
```

**Reported available**: `name, path, size, modified, is_dir, mode, content` (7 listing columns).
**Actually available**: `author, status, tags, title, body` (5 decoded columns).
**Overlap: zero.** Every column named in the error is absent from the relation, and every column in
the relation is absent from the error.

## The masking, measured

The verbatim pipeline dies at `unknown_column` **before** reaching any of the guards that would also
have rejected it. Each had to be isolated separately:

```
$ qfs run "/local<FIX> |> where path like '%.md' |> decode md"
{"error":{"code":"decode_needs_single_blob","kind":"usage",
 "message":"DECODE expects exactly one blob (a single file); got 4 rows"}}
EXIT=2

$ qfs run "/local<FIX> |> where name == 'alpha.md' |> decode md"
{"error":{"code":"decode_needs_blob","kind":"usage","message":"the `content` column is not bytes"}}
EXIT=2

$ qfs run "/local<FIX>/alpha.md |> decode md |> expand tags"
{"error":{"code":"codec_then_query","kind":"usage","message":"querying DECODEd data is not yet
 supported — DECODE/ENCODE must be the final pipeline stages"}}
EXIT=2
```

Three distinct, real rejections, none of which the operator sees while the `unknown_column` error
fires first naming the wrong columns.

### The context that makes the masking matter

- **`decode` is single-blob only.** `packages/qfs/crates/exec/src/codec.rs:133` returns
  `decode_needs_single_blob` for any row count other than one (`:122`/`:146` return
  `decode_needs_blob`).
- **Narrowing to one row does not rescue it.** A `/local` *directory listing* carries `content` as
  **`null`** for every row (verified: the listing above renders `"content":null`); only a direct
  single-file read materializes bytes. So a narrowed set fails with `decode_needs_blob` — *"the
  `content` column is not bytes"* — for a reason unrelated to the row count.
- **`transform` never reaches the exec-side `codec_then_query` guard at all.** It dies earlier, at
  lowering:

  ```
  $ qfs run "/local<FIX>/alpha.md |> decode md |> transform summarize"
  {"error":{"code":"transform_not_executable","kind":"internal",
   "message":"TransformNotExecutable { name: \"summarize\" }"}}
  EXIT=5

  $ qfs run "/local<FIX>/alpha.md |> transform summarize"      # CONTROL: no codec present
  {"error":{"code":"transform_not_executable","kind":"internal",
   "message":"TransformNotExecutable { name: \"summarize\" }"}}
  EXIT=5
  ```

  The control reproduces the identical error **with no codec in the pipeline**, which is what
  establishes the codec is irrelevant to that failure. Mechanism:
  `packages/qfs/crates/pushdown/src/lower.rs:388` —
  `transform_of(&t.name).ok_or_else(|| LowerError::TransformNotExecutable { name: t.name.clone() })?`
  — resolves the transform at lowering, before the exec-side `apply_codecs` guard runs.
  `codec_chain`'s ignore list (`exec/src/codec.rs:57-79`, `Extend | Set | As | Call`) would
  structurally catch `PipeOp::Transform`, but that arm is unreachable for it.

## The recorded reason, and what this ticket does not claim

`exec/src/codec.rs:10-15` records why the planner does not know the decoded schema: it is
**data-dependent and late-bound** — the planner only has the blob source's `describe` schema. That
is the same reason `codec_then_query` exists at all.

**This ticket does not ask the planner to learn the decoded schema.** That is the deep change
(item 3 of the measurement's "what would have to change" list: *"a planner change, not a guard
relaxation"*), and it is the developer's decision, not this ticket's. This ticket is narrower:
**an error must not assert a set of available columns that is false.** If the decoded schema is
unknown at that point, the honest answers include reporting no column list, reporting that the
schema is undetermined after a `decode`, or refusing the stage with `codec_then_query` before the
column check runs — but not silently substituting the input's columns as if they were the
relation's.

## Scope

**In scope:** a pipeline containing a codec stage never reports the **pre-decode** columns as the
relation's available columns. Whatever the chosen shape (no list, an explicit "undetermined after
`decode`", or an earlier structured refusal), the error must not name columns the relation does not
have.

**Out of scope / do not do in passing:**

- **Teaching the planner the decoded schema.** The deep change; explicitly a developer decision.
  `eval.rs:156` is where the wrong schema is *reported*, not where the right one could be *computed*.
- **Lifting `decode_needs_single_blob` or `codec_then_query`.** Both are real, separate constraints
  measured above; leave them.
- **`Filter` / `Shape` reporting `input.schema()`.** Both are schema-preserving; correct as written.
  Only the `Codec` arm is at issue.
- **The `decode md` vs. `/markdown` driver question.** The archived measurement is evidence for a
  developer decision. The driver ships, its mission is 7/7, qfs-viewer PR #11 consumes
  `/markdown/<name>/documents|links`. Nothing here removes, deprecates, or changes it.
- **`transform_not_executable`'s own behavior.** Recorded above as context for the masking (and as
  the control that isolates it); not a defect this ticket fixes.

## Key Files

- `packages/qfs/crates/core/src/eval.rs:145-158` — the `PlanSource::schema()` match; the `Codec`
  arm at `:156` is the defect site.
- `packages/qfs/crates/core/src/eval.rs:847-850` — where `PlanSource::Codec` is constructed
  (`PipeOp::Decode(codec) | PipeOp::Encode(codec)`), carrying `input` + `fmt` and no output schema.
- `packages/qfs/crates/exec/src/codec.rs:10-15` — the recorded reason the decoded schema is
  late-bound.
- `packages/qfs/crates/exec/src/codec.rs:57-89,122,133,146` — `codec_then_query`,
  `decode_needs_blob`, `decode_needs_single_blob`.
- `packages/qfs/crates/pushdown/src/lower.rs:388` — `TransformNotExecutable`, the lowering that
  fires before the exec-side codec guard.
- `packages/qfs/crates/codec/src/codecs/markdown.rs:43-69` — what `decode md` actually produces.

## Policies

- `workaholic:design` — 「推測するな、宣言して拒否せよ」. An error that does not know the schema must
  say so; substituting the input's columns is a guess presented as a declaration.
- `workaholic:implementation` / `objective-documentation` — a diagnostic is a factual claim about the
  running system. `available: [...]` naming a zero-overlap set is a false statement emitted by the
  binary.
- `workaholic:implementation` — the failure **order** is part of the behavior: an error raised at
  lowering hides the guards behind it, so a diagnostic that fires first must be correct or must not
  fire.
- `workaholic:development` / `qa-engineering` — verified by a both-directions test, not by review.

## Quality Gate

Verify with **raw exit codes** — `echo "EXIT=$?"` immediately after each command. Never `cmd | tail`
or `|| true`.

1. **The pre-decode columns are no longer reported as available.** Run, verbatim:
   `/local<FIX>/alpha.md |> decode md |> expand front_matter |> transform triage`. The error does not
   list `name, path, size, modified, is_dir, mode, content` as the relation's available columns.
   Actual command, output, exit code pasted.
2. **The chosen shape is stated and justified.** Record which answer was taken — no column list, an
   explicit "schema undetermined after `decode`", or an earlier structured refusal — and why, in the
   ticket outcome and the commit body. Do **not** resolve it by teaching the planner the decoded
   schema (out of scope, developer's decision).
3. **Both directions.** A test that fails on the current code (asserting the wrong columns are
   reported today) and passes after.
4. **The real constraints still fire, unchanged**, each shown by a run with its exit code:
   - `/local<FIX> |> where path like '%.md' |> decode md` → `decode_needs_single_blob`
   - `/local<FIX> |> where name == 'alpha.md' |> decode md` → `decode_needs_blob`
   - `/local<FIX>/alpha.md |> decode md |> expand tags` → `codec_then_query`
5. **A clean `decode` is untouched.** `/local<FIX>/alpha.md |> decode md` still returns the decoded
   relation (`author, status, tags, title, body`; `row_count: 1`) at exit 0 — pinned by a test.
6. **`Filter` and `Shape` are untouched.** Both still report `input.schema()`; pinned by a test that
   would fail if the fix over-reached to the schema-preserving arms.
7. **The masking is measured after the fix.** State what the verbatim pipeline now reports first,
   and whether the guards behind it are now reachable. If a different error still masks the others,
   say so plainly rather than declaring the pipeline fixed.
8. **Workspace gates green, raw exit codes shown**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --workspace`, plus `cargo run -p xtask --
   gen-docs --check` if any describable surface moved. Patch bumped per CLAUDE.md when this reaches
   a PR.

## Considerations

- The measurement this came from records the general lesson: *"The failure ORDER matters as much as
  the failure."* This ticket is that lesson applied to one site — the first error a codec pipeline
  raises is the one an operator acts on, so it carries the weight of all the errors it hides.
- Sibling tickets filed alongside: `20260717180100` (`where` on an unknown column returns 0 rows at
  exit 0) and `20260717180200` (`expand` silently no-ops). All three surfaced from the same
  measurement. This one differs in kind from the other two: it produces a **visible error**, so an
  operator knows something failed — the defect is that the error's content is false. The other two
  produce no signal at all.
- `eval.rs:156` is a three-way `|` arm (`Filter | Shape | Codec`). The fix must split `Codec` out
  rather than change the arm's behavior for all three.
