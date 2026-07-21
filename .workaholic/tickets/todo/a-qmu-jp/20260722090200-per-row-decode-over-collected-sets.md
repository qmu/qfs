---
created_at: 2026-07-22T09:02:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260722090100-design-brief-codec-relation-surface-and-13b-ruling.md]
mission: a-file-collection-is-a-declared-set-over-any-blob-source
---

# DECODE runs per row over a collected set; the single-blob refusal retires

## Overview

Mission acceptance items 1–3 (the owner's three verbatim requirements). Today a glob/directory
listing carries no `content` column (`driver-local/src/read.rs:46`) and `DECODE` rejects any
batch that is not exactly one row (`exec/src/codec.rs:111-140`, `decode_needs_single_blob`) —
so every multi-file `decode` recipe the cookbook teaches is rejected by the binary. This ticket
makes the taught pipeline the executable truth:

1. **A collected set feeding a decode materializes each file's bytes** — plan-driven: the
   engine knows a decode follows the collect and materializes `content` per row for the
   segment. Re-verify the current source first; line references above may have drifted.
2. **`DECODE` applies the codec's `bytes↔rows` contract to each row's bytes** of a multi-row
   content-bearing set, and the per-file relations union. `decode_needs_single_blob` retires;
   the single-file case passes as the one-row instance of the same rule.
3. **Provenance rides the decode application**: every decoded row carries the root-relative
   `path` column (the canonical join id), regardless of codec — per the design brief's
   contract (depends_on).

## Hermetic proof (the acceptance's verbatim gates)

- A `/local` mount over a fixture tree is the only mechanism: a hermetic engine-level test
  collects the tree through the mount and the pipeline alone (item 1).
- A `*.md` glob decodes to one row per file; a `*.json` and a `*.yaml` set decode the same
  way; every decoded row carries `path` (item 2).
- `… *.md |> decode md |> where <front-matter key> == …` over the fixture tree returns exactly
  the matching files' rows; a file missing the key reads as null, not an error (item 3).

## Policies

- Runtime-semantic hard break, sanctioned by the mission: this is a redefinition of `DECODE`,
  not a migration. Do not keep a compatibility path for the single-blob refusal.
- All proof is hermetic (fixture trees under the test tree); no live cloud sources.
- The provenance and per-row contracts come from the design brief — implement what it ruled;
  overturn only with cause recorded in the mission changelog.

## Quality Gate

- The hermetic tests above land in the workspace suite and pass; removing the plan-driven
  materialization or re-adding the single-blob refusal makes at least one fail.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check` all pass.
- The language docs regenerate if the DECODE surface description changes (never hand-edited).
