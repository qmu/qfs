---
created_at: 2026-07-11T12:15:31+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: [20260711121529-live-model-providers-anthropic-openai-google.md]
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Transformation chains: multiple transform stages composed in one pipe

## Overview

Prove and harden **chained transforms**: `… |> transform a |> transform b |> …` where stage b's
INPUT schema is fed by stage a's OUTPUT schema (e.g. extract → summarize → classify). The plan
spine's schema fold was built to expose OUTPUT downstream, so chaining may already plan — but it
has never been exercised deliberately: unknowns are per-stage forced-local planning stacking,
mode derivation on an upstream transform's output, PREVIEW purity across two model-free stages,
and the commit envelope when two irreversible-gated model calls ride one statement. This ticket
pins the semantics with tests, fixes what breaks, and lands the taught chain recipe.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/type-driven-design.md` — chain compatibility is a schema-unification check at plan time: a's OUTPUT must satisfy b's INPUT or the statement fails at PREVIEW
- `workaholic:implementation` / `policies/functional-programming.md` — each stage stays a pure planned node; effects only at the commit boundary

## Key Files

- `packages/qfs/crates/pushdown/src/lower.rs` - transform lowering + schema fold (the chain's plan-time seam)
- `packages/qfs/crates/qfs/src/transform.rs` - executor; sequential stage execution and per-stage provider calls
- `packages/qfs/crates/types/src/transform.rs` - derive_mode over an upstream transform's OUTPUT
- `packages/qfs/crates/pushdown/tests/lowering.rs` - existing lowering tests to extend with chain cases

## Related History

- [20260708192731-transform-plan-spine.md](.workaholic/tickets/archive/work-20260709-023822/20260708192731-transform-plan-spine.md) - the OUTPUT schema fold chaining relies on
- [20260708192732-transform-execution-routing.md](.workaholic/tickets/archive/work-20260709-023822/20260708192732-transform-execution-routing.md) - execution routing the chain executes through

## Implementation Steps

1. Plan-time: add chain lowering tests — compatible chain plans, incompatible (a.OUTPUT ⊄ b.INPUT) fails at PREVIEW with a schema-diff error; mixed chains (transform → where → transform) fold correctly.
2. Execution: mock-provider chain test asserting stage order, per-stage ModelRequest correctness, and that stage b receives exactly stage a's rows; failure mid-chain aborts the whole statement (no partial commit).
3. Rule and record the gate semantics: one `--commit` covers the whole chain's model calls (they are the statement's effect), PREVIEW stays model-free for every stage.
4. Cookbook chain recipe (extract → summarize) parse-checked; regenerate docs/skills.

## Quality Gate

**Acceptance criteria**

- A two-stage chain plans, previews model-free, and executes in order against mock providers with schema-checked handoff.
- An incompatible chain fails at PREVIEW with a structured schema error naming both schemas.
- Mid-chain provider failure aborts atomically.

**Verification method**

- `cargo test --workspace` green including new lowering + execution chain tests; `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; the live chained round (real keys, two real stages) runs owner-attended and is recorded on this ticket.

## Considerations

- Two model calls in one statement double the live cost — the recipe should use small models and capped tokens (docs)
- If forced-local stacking degrades pushdown around the chain, record the plan shape rather than optimizing here (`packages/qfs/crates/pushdown/src/planner.rs`)

## Live Round Evidence

### Round 6 — two-real-stage transform chain (2026-07-13, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a). Two stages, both anthropic / claude-haiku-4-5-20251001 /
  effort low / `secret 'env:ANTHROPIC_API_KEY'`: `sumline` (subject → summary) then `digestline`
  (summary → digest).
- **Schema handshake proven negatively first:** piping the source straight into `digestline`
  refused at preview, model-free, naming the missing column
  (`TransformInputMissing { digestline, summary }`).
- **The committed chain:** `/local/<scratch>/round5 |> select name as subject |> limit 2
  |> transform sumline |> transform digestline` → returned a final `digest` text row produced by
  two real chained Anthropic calls under one consent ack (`affected 2` = the two transform
  calls). The digest text was a genuine model composition over the stage-1 summaries.
- **Defect found (ticketed 20260713123000):** the same chain sourced from `/mail/inbox` previews
  fine but fails at commit with "READ is not serviced by the Gmail driver" — a read-terminal
  transform plan services the source READ through a facet Gmail lacks, though the round-2 switch
  commit path reads Gmail fine. The /local source was the workaround.
- **Residue:** transform definitions `sumline`, `digestline` (plus round 5's `extractpdf`,
  `extractpdf2`); `remove transform <name>` drops each.
