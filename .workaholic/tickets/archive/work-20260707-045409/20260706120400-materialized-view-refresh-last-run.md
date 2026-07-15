---
created_at: 2026-07-06T12:04:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 4h
commit_hash: 29e627f
category: Added
depends_on: []
---

# Materialized-view refresh: run the stored query and stamp last_run

## SKIPPED in the autonomous batch (2026-07-06) — needs an owner design decision

Verified in-batch: there is **no view refresh/materialize execution path at all** (the
`(EffectKind::*, ServerNode::Views)` branch only stores an incoming `last_run`; nothing computes
one). Stamping `last_run` meaningfully requires first *building the materialization engine*, and its
shape is an **architectural decision the owner should make**, not an autonomous implementation
choice — it touches the blueprint §14 "freshness as data" contract:

1. **Execution model** — where does a refresh run? A CLI `qfs view refresh <name>` (on-demand,
   mirroring `job run`)? A server-side trigger? Both?
2. **Caching + read path** — where is the materialized result stored, and how does a READ of a
   materialized view serve the cache vs. re-run? (A `MATERIALIZED` view's whole point is a served
   cache; `last_run` is only meaningful if a cache exists to be stale.)
3. **Minimal vs full** — is "refresh = re-run the stored query + record `last_run`" (freshness
   observability without a served cache) an acceptable first step, or must it cache-and-serve?

Recommend the owner resolve (1)–(3) in a short design brief (per the design-decision policy), then
this ticket implements the chosen model + the `last_run` stamp + fixes the stale `state.rs` "t32
scheduler" doc comments. The other six triage tickets were implemented autonomously; only this one
carries a genuine design fork.

## What's wanted

`last_run` is a readable column on `/server/views` (honest `null`), but nothing ever writes it —
there is no materialize/refresh execution path for views at all. Build the refresh step (run a
view's stored query, materialize/cache the result) and stamp the current time into that view's
`/server/views` config row's `last_run` on success, so clients can compute staleness.

## Current state (verified against HEAD 61f696c)

- `last_run` read/exposed: `crates/server/src/state.rs:117-121` (`ViewDef.last_run`);
  `crates/server/src/driver.rs:353-366` decodes an incoming row's `last_run` but never computes one.
- No refresh engine: grep for materialize/refresh across qfs/cmd/host/watchtower finds only doc
  references. The only non-null `last_run` in the tree is a hand-built row in the round-trip test
  (`crates/server/src/tests.rs:79-102`).
- `crates/qfs/src/job.rs::run_job` (39-140) is the closest "fire" analogue (builds plan, gates,
  `apply_plan`) but never writes back a fire time; its module doc says the internal scheduler daemon
  is retired ("qfs is not a scheduler"). Stale "t32 records it" doc comments live in `state.rs`.

## Implementation steps

1. Add a view-refresh path: run the view's stored query, materialize/cache the result.
2. On success, `UPSERT /server/views` stamping `last_run` into the view's config row.
3. Expose it (CLI `qfs view refresh` and/or a server refresh binding), mirroring `run_job`.
4. Fix the stale "t32 scheduler" doc comments in `state.rs`; add an automatic-stamp test modeled
   on `tests.rs:79-102`.

## Key files

- `crates/server/src/state.rs`, `crates/server/src/driver.rs`, `crates/qfs/src/job.rs` (analogue),
  `crates/server/src/tests.rs`.

## Considerations

- Larger than "just stamp a timestamp": the refresh execution engine does not exist yet (unlike
  jobs, which at least have `run_job`/`apply_plan`).
- Source concern: `.workaholic/concerns/18-materialized-view-freshness-recording-is-not.md`.

## Final Report

Development completed as a minimal explicit refresh path rather than a hidden scheduler. The server
runtime now executes a materialized view's stored query through an injected read executor, caches the
returned rows, and stamps `last_run` only after a successful read. The CLI exposes this as
`qfs view refresh`, with table/JSON output and docs covering the external-scheduler model.

### Discovered Insights

- **Insight**: `last_run` belongs to the same transaction-shaped server state update as the cached
  rows; stamping freshness without storing the snapshot would recreate the original observability
  gap.
  **Context**: Future refresh work should preserve the "success writes cache + freshness together"
  shape even if refresh later moves behind an HTTP or trigger binding.
