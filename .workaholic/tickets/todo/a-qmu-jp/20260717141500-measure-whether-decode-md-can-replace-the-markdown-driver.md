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
