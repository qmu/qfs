---
created_at: 2026-07-17T14:15:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# Measure whether `decode md` can replace the `/markdown` driver

## Overview

**This ticket is a MEASUREMENT, not a rewrite.** Its output is evidence for a developer decision.
It deliberately QUESTIONS a design that is already shipped and consumed downstream (see
*The tension* below); nothing here authorizes deleting or deprecating the `/markdown` driver.

Developer direction (design discussion, 2026-07-17): **markdown should NOT be a built-in
resource.** Instead, a set of files gathered from `/local` (or any source) under a condition
should be registerable as an *aliased resource*, and the markdown-ness should fall out of a
**stage**, not a driver. In their words:

> 集めてきたマークダウンファイルのフロントマターなどメタ情報を decode して展開するところまでは
> qfs-query で表現できるようにしたい、そこから LLM にパイプするなどもあり得る。

The direction is architecturally coherent with the shipped grammar — "a set defined by a
condition" needs **no new syntax** (see *Verified against source*). The question is what the
existing stages actually do today, and how far the gap runs.

## Verified against source (2026-07-17, this session)

Everything below was read in the tree at `origin/main`; file:line citations are load-bearing.

1. **A condition-defined set is already expressible.** `docs/language.md:232` —
   `ddl = "create" , ( endpoint | trigger | job | view | webhook | policy | transform_def ) ;`
   and `:236` — `view = ( "view" | "materialized view" ) , name , "as" , pipeline ;`. A `view`
   over a filtered pipeline IS the aliased resource; no grammar change is needed to name a set.
   (Note: the last DDL alternative renders as `transform_def`, not `transform`.)
2. **`md` is in the grammar.** `docs/language.md:204-205` —
   `codec_stage = "decode" , format | "encode" , format ;` and
   `format = "json" | "jsonl" | "yaml" | "toml" | "csv" | "md" | "multipart" ;`.
3. **`md` is ALSO wired** — `packages/qfs/crates/codec/src/codecs/markdown.rs:28`
   `MarkdownFrontmatterCodec`, whose `fmt()` returns `"md"` (`:31-33`); it is included in
   `builtin_codecs()` (`codec/src/codecs/mod.rs:38`), the single source of truth
   `CodecRegistry::with_builtins()` loads; and `codec/tests/codecs.rs:460`
   (`builtin_codecs_cover_all_six_formats`) asserts all six names **including `md`**.
   *A grep for registered codec name literals finds only `"json"`/`"yaml"` and misses this —
   codecs declare their name in the `fmt()` method body, not in a registry string list. The
   "md may be grammar-only" suspicion is FALSE.*
4. **`expand` exists** — `docs/language.md:140` (`| "expand" , column`), `PipeOp::Expand(PathRef)`
   at `parser/src/ast.rs:207`, lowered via `Schema::expand` at `core/src/eval.rs:838-846`.
5. **`transform <name>` is the model-calling stage** — `parser/src/ast.rs:214`, a **contextual
   identifier**, not a frozen keyword (`docs/language.md:146`, closed core stays 39). It is
   effect-bearing: `docs/language.md:149-150` — "A transform-bearing statement is effect-bearing:
   it previews (no model call) and commits (the model runs) through the plan_op gate, and its
   commit is irreversible."

### What the source already indicates (CONFIRM by real runs; do not take on trust)

These are read-throughs, not measurements. The Quality Gate below demands live evidence.

- **(a) `decode md` returns a FLAT, SINGLE-ROW relation** — not the documents/links two-table
  shape. `codecs/markdown.rs:43-69`: frontmatter keys become **top-level columns**, the body
  becomes a `body` Text column, and the result is `RowBatch::new(schema, vec![Row::new(values)])`
  — exactly one row, always.
- **The crux: `links` does NOT come out of a stage.** `grep -rl source_section_path` over
  `packages/qfs/crates/` hits ONLY `qfs/src/markdown.rs`, `qfs/src/describe.rs`, and
  `driver-markdown/src/{parse,schema,lib}.rs` — **zero hits in the codec crate**. The codec has no
  heading stack at all; the ATX-heading stack that produces the heading-as-field crossing edges
  lives in `driver-markdown/src/parse.rs`. **If the measurement confirms this, that gap is the
  only real implementation target** — everything else in the developer's pipeline is composition.
- **(b) `decode` does NOT apply per-row across a set — it hard-errors.** `exec/src/codec.rs:113-140`
  (`blob_bytes`) requires a `content` column (else `decode_needs_blob`) **and** matches
  `batch.rows.as_slice()` against `[only]`; any other row count returns `decode_needs_single_blob`
  — *"DECODE expects exactly one blob (a single file); got N rows"*. So N files under `/local` →
  N rows → decode **fails**. Composition with `expand` does not rescue this (see (d)).
- **(c) Answered above: `md` IS wired.** Re-confirm live rather than by grep.
- **(d) The proposed pipeline cannot be written today** — it fails at **two independent** points:
  1. `where path like '%.md'` over a directory yields N rows → `decode_needs_single_blob`.
  2. **Any relational op after a codec is rejected** — `exec/src/codec.rs:59-89` (`codec_chain`)
     returns the usage error `codec_then_query`: *"querying DECODEd data is not yet supported —
     DECODE/ENCODE must be the final pipeline stages"*. The ignore list at `:75` is exactly
     `Extend | Set | As | Call` — **`Expand` and `Transform` are NOT in it**, so both
     `|> expand front_matter` and `|> transform <t>` trip the guard. The recorded reason
     (`exec/src/codec.rs:10-15`): the decoded schema is data-dependent and late-bound; the planner
     only knows the blob source's `describe` schema. Corroborated by `core/src/eval.rs:156`, where
     `PlanSource::Codec` reports `input.schema()` — the plan never learns the decoded shape.
  3. Third, smaller: **there is no `front_matter` column to expand.** `decode md` splats
     frontmatter keys as top-level columns (`markdown.rs:52-62`). The nested `frontmatter` Json
     column that `expand` would target exists only on the DRIVER's `documents` table
     (`driver-markdown/src/schema.rs:94`) — i.e. on the very thing this direction would replace.

So the source suggests **three distinct gaps** stand between the direction and today: decode is
single-blob; codecs must be the pipeline tail; and links/section-path exist only in the driver.
The measurement's job is to confirm or refute each with real runs.

## The tension this ticket must record honestly

The qfs mission `markdown-trees-are-queryable-as-documents-and-links-tables` is **7/7 acceptance
items ticked** and its first slice **merged as PR #6** (`MERGED 2026-07-17T00:04:24Z`, *"Markdown
trees resolve as documents and links tables through the engine"*). qfs-viewer's **open PR #11**
(`qmu/qfs-viewer`, *"Read the corpus from qfs's collection path behind one collection switch"*)
consumes `/markdown/<name>/documents` and `/markdown/<name>/links` — verified in its diff.

*(Precision: the mission's 7 items are all ticked, but its frontmatter `status:` is still `active`
and it sits in `.workaholic/missions/active/` — it has not been formally closed.)*

**This ticket questions that shipped design. It must not silently invalidate it.** Its outcome is
**evidence for a developer decision**, never a mandate to delete the driver. A finding of "the
stage cannot do X" is as valuable as the opposite; record what is true, and stop there. If the
measurement argues for a change, that change is a separate ticket the developer authorizes.

## Key Files

- `packages/qfs/crates/exec/src/codec.rs` — the `codec_then_query` and `decode_needs_single_blob`
  guards; the whole of (b) and (d) lives here.
- `packages/qfs/crates/codec/src/codecs/markdown.rs` — what `decode md` actually returns.
- `packages/qfs/crates/codec/src/codecs/mod.rs` — `builtin_codecs()`, the registry seam.
- `packages/qfs/crates/core/src/eval.rs:156` — `PlanSource::Codec` reports the INPUT schema.
- `packages/qfs/crates/driver-markdown/src/{parse,schema}.rs` — where `links` /
  `source_section_path` are actually produced today.
- `docs/language.md` — the frozen grammar (`view`, `codec_stage`, `expand`, `transform_stage`).

## Policies

- `workaholic:implementation` — measure the running system, do not reason from the grammar alone;
  the grammar admitting `md` said nothing about whether a pipeline runs.
- `workaholic:design` — "declare, don't guess; refuse the undeclared". A finding must be evidence,
  not an inference, before it can move a shipped, consumed design.
- `workaholic:planning` — a shipped mission and a downstream open PR are stakeholders; a
  measurement that touches them reports, it does not decide.

## Quality Gate

**Every answer below requires real query runs with RAW exit codes** (`echo "EXIT=$?"` immediately
after the command — never `cmd | tail`, which masks the exit code). Paste the actual command, the
actual stdout/stderr, and the actual exit code. A source citation alone does NOT satisfy this gate.

1. **(a) What does `decode md` return today?** Run it against a real `.md` file with frontmatter.
   Record the exact schema and row count. State plainly whether it is the documents/links two-table
   shape or a flat one-row-per-document relation.
2. **(a-crux) Does `links` come out of a stage at all?** Demonstrate, by run, whether any stage
   composition yields the heading-as-field crossing edges with `source_section_path`. **This is the
   crux**: if it does not, say so explicitly and name that gap as the only real implementation
   target. Do not propose the implementation here.
3. **(b) Does `decode` apply per-row across a SET?** Run `/local/<dir> |> where path like '%.md'
   |> decode md` over a directory holding ≥2 `.md` files. Record the verbatim error or the rows.
   Then test whether composition with `expand` changes the answer, and report the result.
4. **(c) Is `md` wired, or grammar-only?** Settle it by run, not by grep. If the answer is "wired"
   (as the source indicates), say so and record that the original suspicion was false.
5. **(d) Can the developer's pipeline be written today, and what does it produce?** Run verbatim:
   `/local/<path> |> where path like '%.md' |> decode md |> expand front_matter |> transform <t>`
   Record every failure point separately (the pipeline may fail more than once for unrelated
   reasons — report each, not just the first). If it cannot be written, state the minimal set of
   changes that would let it — as a FINDING, not as a plan of record.
6. **The tension is recorded honestly in the ticket's outcome**: the finding names PR #6 (merged),
   qfs-viewer PR #11 (open, consuming `/markdown/<name>/documents|links`), and states explicitly
   that the outcome is evidence for a developer decision — not a mandate to remove the driver.
7. **No production behavior changes in this ticket.** If any code is touched (e.g. a scratch
   fixture), it is not committed. `cargo test --workspace`, clippy `-D warnings`, and `cargo fmt
   --all --check` remain green — verified with raw exit codes.

---

## Outcome — measured 2026-07-17 (branch `work-20260717-160001`, binary `qfs 0.0.77`, commit `f895c4a`)

**Verdict: `decode md` CANNOT replace the `/markdown` driver today.** Not by a small margin, and
not only for the reasons the source reading predicted. **This is evidence for a developer decision;
it is NOT a mandate to remove, deprecate, or change the driver** (see *The tension*, re-affirmed
below).

Method: two-file fixture (`alpha.md`, `beta.md`, each with YAML frontmatter, ATX headings, and
markdown links between them) in a scratch dir outside the repo. Every run below is verbatim, with a
raw `echo "EXIT=$?"`. No production code was touched. Two scratch `path_binding` rows
(`/markdown/t1measure`, `/markdown/t1b`) were created to observe the driver and **removed**;
`/sys/connections` verified back to its original 10 rows.

### Claim-by-claim

| # | Claim under test | Result |
|---|---|---|
| (c) | `md` is wired (not grammar-only) | **CONFIRMED** |
| (a) | `decode md` → flat single row | **CONFIRMED** |
| (b) | `decode` is single-blob only | **CONFIRMED — and worse than stated** |
| (a-crux) | `links` does not come out of a stage | **CONFIRMED** |
| (d) | Pipeline fails in three independent places | **PARTIALLY REFUTED — the mechanism differs** |

### (c) + (a) — `md` IS wired, and returns a flat single row

```
$ qfs run "/local<FIX>/alpha.md |> decode md"
{"schema":[{"name":"author","type":"text"},{"name":"status","type":"text"},
{"name":"tags","type":"array"},{"name":"title","type":"text"},{"name":"body","type":"text"}],
"rows":[{"author":"a@qmu.jp","status":"active","tags":["one","two"],"title":"Alpha Document",
"body":"\n# Alpha Heading\n\nBody text of alpha, ..."}],
"meta":{"row_count":1,...}}
EXIT=0
```

Exit 0, real decoded rows: **`md` is wired**. The earlier "grep only shows json/yaml" suspicion was
an artifact of codecs declaring their name inside `fmt()`; recorded here as **FALSE**.

The shape is a **flat, single-row relation** (`row_count: 1`): frontmatter keys splatted to
top-level columns + a `body` Text column. It is **not** the documents/links two-table shape. There
is no `links` column, no `source_section_path`, and **no `front_matter` column**.

### (b) — `decode` is single-blob only, and the set path fails at TWO layers (new finding)

```
$ qfs run "/local<FIX> |> where path like '%.md' |> decode md"
{"error":{"code":"decode_needs_single_blob","kind":"usage",
"message":"DECODE expects exactly one blob (a single file); got 2 rows"}}
EXIT=2
```

Confirmed verbatim. **But the single-blob constraint is not the only barrier.** Narrowing the set to
exactly one row does *not* rescue it — it fails differently:

```
$ qfs run "/local<FIX> |> where path like '%.md' |> limit 1 |> decode md"
{"error":{"code":"decode_needs_blob","kind":"usage","message":"the `content` column is not bytes"}}
EXIT=2
$ qfs run "/local<FIX> |> where name == 'alpha.md' |> decode md"
{"error":{"code":"decode_needs_blob","kind":"usage","message":"the `content` column is not bytes"}}
EXIT=2
```

**Why:** a `/local` *directory listing* carries `content` as **`null`** for every row (verified: the
listing renders `"content":null` on both rows). Only a *direct single-file* read materializes bytes.
So "gather a set of files under a condition, then decode them" is blocked at **two independent
layers**: the row-count guard **and** byte materialization. Lifting `decode_needs_single_blob` alone
would not make the developer's direction work — the set-shaped path never carries bytes at all.
The source reading did not predict this second layer.

### (a-crux) — `links` does NOT come out of any stage. This is the only real implementation target.

Zero hits for `source_section_path` in the codec crate (`grep -rn source_section_path
packages/qfs/crates/codec/` → `EXIT=1`, no output); it lives only in `driver-markdown/` and
`qfs/src/{markdown,describe}.rs`. Confirmed live — no stage composition yields the crossing edges.
Only the **driver** does:

```
$ qfs run "/markdown/t1measure/links"
{"schema":[{"name":"source_doc","type":"text"},{"name":"source_section_path","type":"array"},
{"name":"target","type":"text"},{"name":"target_doc","type":"text"},{"name":"line","type":"int"}],
"rows":[{"source_doc":"alpha.md","source_section_path":["Alpha Heading"],"target":"./beta.md",...},
{"source_doc":"alpha.md","source_section_path":["Alpha Heading","Alpha Subsection"],...},
{"source_doc":"beta.md","source_section_path":["Beta Heading"],...}],"meta":{"row_count":3,...}}
EXIT=0
```

The driver produces the **heading stack** (`["Alpha Heading","Alpha Subsection"]`) and resolves
edges **across** files — over **2 documents in one query** (`documents` → `row_count: 2`), which
`decode` structurally cannot do at all. **The ATX-heading stack is the gap, and it is the only real
implementation target.** Per this ticket's scope, no implementation is proposed here.

### (d) — The pipeline cannot be written. The failure count is right; the mechanism is NOT.

Verbatim run:

```
$ qfs run "/local<FIX> |> where path like '%.md' |> decode md |> expand front_matter |> transform summarize"
{"error":{"code":"unknown_column","kind":"usage","message":"Type(UnknownColumn { name: \"front_matter\",
available: [\"name\", \"path\", \"size\", \"modified\", \"is_dir\", \"mode\", \"content\"] })"}}
EXIT=2
```

The **first** failure is `unknown_column` at *lowering*, and the `available` list is the **blob
source's listing columns** — a live confirmation of `core/src/eval.rs:156` (`PlanSource::Codec`
reports `input.schema()`): the planner never learns the decoded shape. This failure **masks** the
others, so each was isolated:

1. **Single-blob constraint** — CONFIRMED (above).
2. **`expand` trips `codec_then_query`** — CONFIRMED:
   ```
   $ qfs run "/local<FIX>/alpha.md |> decode md |> expand front_matter"
   {"error":{"code":"codec_then_query","kind":"usage","message":"querying DECODEd data is not yet
   supported — DECODE/ENCODE must be the final pipeline stages"}}
   EXIT=2
   ```
3. **`transform` trips `codec_then_query`** — **REFUTED.** It never reaches that guard:
   ```
   $ qfs run "/local<FIX>/alpha.md |> decode md |> transform summarize"
   {"error":{"code":"transform_not_executable","kind":"internal",
   "message":"TransformNotExecutable { name: \"summarize\" }"}}
   EXIT=5
   ```
   Control — the **same error without any codec in the pipeline**, proving the codec is irrelevant
   to it:
   ```
   $ qfs run "/local<FIX>/alpha.md |> transform summarize"   → transform_not_executable, EXIT=5
   ```
   With a **registered** transform (`triage`, already in `/transform`; run in preview, **no model
   call, no cost**), it still never reaches the guard:
   ```
   $ qfs run "/local<FIX>/alpha.md |> decode md |> transform triage"
   {"error":{"code":"transform_input_missing","kind":"internal",
   "message":"TransformInputMissing { name: \"triage\", column: \"subject\" }"}}
   EXIT=5
   ```
   **Mechanism:** transform is resolved at **lowering** (`pushdown/src/lower.rs:388`), which runs
   *before* the exec-side `apply_codecs` guard, and its input-column check reads the **pre-decode**
   schema. `codec_chain`'s ignore list (`Extend|Set|As|Call`) *would* structurally catch
   `PipeOp::Transform`, but that arm is unreachable for it — the lowering always errors first. So
   the claim "both `expand` and `transform` trip `codec_then_query`" is **half right**: `expand`
   does; `transform` fails earlier, for a different reason, with a different code and exit status.
4. **No `front_matter` column** — CONFIRMED, with a correction: the driver's Json column is spelled
   **`frontmatter`** (one word), not `front_matter` (`documents` schema, verified live). So the
   pipeline names a column that exists on **neither** path.

### New finding (not in the source reading): `expand` silently no-ops on Json and on unknown columns

`Schema::expand` (`types/src/schema.rs:322-349`) documents `UnknownColumn` if the field is absent
and `NotExpandable` for a `Json`/scalar column. **Neither fires on a pure read.** Measured:

```
$ qfs run "/markdown/t1b/links |> expand source_section_path"     → EXIT=0, 3 rows -> 4 rows,
   source_section_path: array -> text   # a real Array column: expand WORKS
$ qfs run "/markdown/t1b/documents |> expand frontmatter"          → EXIT=0, schema UNCHANGED
   (frontmatter still "type":"json")    # a Json column: SILENT NO-OP, no NotExpandable
$ qfs run "/markdown/t1measure/documents |> expand front_matter"   → EXIT=0, schema UNCHANGED
   # a column that does not exist at all: SILENT NO-OP, no UnknownColumn
```

The validation only fires when a downstream `transform` forces schema resolution at lowering:

```
$ qfs run "/local<FIX> |> expand nosuchcol"                  → EXIT=0, silently unchanged
$ qfs run "/local<FIX> |> expand nosuchcol |> transform triage"
{"error":{"code":"unknown_column",...,"available":["name","path","size",...]}}   EXIT=2
```

A related silent-wrong-answer, and **general — not markdown-specific** (reproduced identically on
`/local`, `/markdown`, and `/sys`):

```
$ qfs run "/markdown/t1measure/documents |> where nosuchcol == 'x'"  → EXIT=0, row_count: 0
$ qfs run "/local<FIX> |> where nosuchcol == 'x'"                    → EXIT=0, row_count: 0
$ qfs run "/sys/connections |> where nosuchcol == 'x'"               → EXIT=0, row_count: 0
```

A filter on a **column that does not exist** returns an empty result with exit 0 rather than
`unknown_column`. An empty relation is an answer an operator will read as *"nothing matched"*.

**Why this matters to the direction, beyond the bug itself:** the developer's stated goal is to
`decode` frontmatter **and expand it** (「フロントマターなどメタ情報を decode して展開する」).
Expanding a frontmatter blob into columns is **not implemented for `Json` on either path** — the
codec path splats keys itself (no expand needed, but it is single-blob), and the driver path carries
`frontmatter` as a Json column that `expand` silently ignores. This is a **fourth gap**, and it sits
squarely on the developer's intended route.

These are recorded as findings only. **No fix is attempted here** — this ticket changes no
production behavior, and each is its own ticket if the developer wants them.

### What would have to change (FINDING, not a plan of record)

For `decode md` + stages to reach parity with the driver, at minimum:

1. **Per-row decode over a set** — lift `decode_needs_single_blob` (`exec/src/codec.rs:128-139`).
2. **Byte materialization for set-shaped reads** — a listing's `content` is `null`; a set path must
   carry bytes before any per-row decode is meaningful. *(Not previously identified.)*
3. **Relational ops after a codec** — `codec_then_query` (`exec/src/codec.rs:59-89`) requires the
   planner to learn a **late-bound, data-dependent** decoded schema (`eval.rs:156`). This is the
   deep one: it is a planner change, not a guard relaxation.
4. **A heading-stack / links producer as a stage** — the ATX stack in `driver-markdown/src/parse.rs`
   has no codec-side equivalent. **The crux.**
5. **`expand` over `Json`** — otherwise the "展開する" half of the direction has no operator.

Items 3 and 4 are substantial. The direction is architecturally coherent (a `view` over a filtered
pipeline needs no new grammar — `docs/language.md:236`), but the distance from today is **five
gaps**, not the one the framing implied.

### The tension, recorded honestly

- The qfs mission `markdown-trees-are-queryable-as-documents-and-links-tables` is **7/7 acceptance
  items ticked**, its first slice **merged as PR #6** (2026-07-17). *(Its frontmatter `status:` is
  still `active` in `.workaholic/missions/active/` — not formally closed.)*
- **qfs-viewer PR #11** (`qmu/qfs-viewer`, open) consumes `/markdown/<name>/documents` and
  `/markdown/<name>/links`.
- The driver **works**, measured live in this session: it produces heading stacks and cross-file
  edges over a multi-document set — capabilities `decode md` does not have at any composition.

**This outcome is evidence for a developer decision, not a mandate to remove the driver.** Nothing
here was removed, deprecated, or broken. If the measurement argues for a change, that change is a
separate ticket the developer authorizes.

### Gate

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace` — raw exit codes recorded in the commit for this ticket. No production code
changed; no version bump (this ticket ships no code).
