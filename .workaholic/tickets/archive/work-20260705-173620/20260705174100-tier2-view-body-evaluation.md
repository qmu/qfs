---
created_at: 2026-07-05T17:41:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
commit_hash: a86977c
category: Added
depends_on: [20260705174000-tier2-transport-honesty-cursor-path-redirect.md]
---

# §13 tier-2 (2/3): view-body evaluation — a declared view IS its stored query

## Overview

The core of the blueprint §13 **Tier 2** decision. Today `RestReadDriver::scan`
(`read_facets.rs`) ignores the stored view body and fetches the wire natively — the root of
parity gaps ① (envelope), ③ (typing), ④ (path/endpoint coupling). Replace that with **body
evaluation**: reading a declared mount parses the stored body, checks confinement at plan
time, fetches through the confined transport, executes the remaining pipe ops through the
real engine operators, applies the `OF` type as a shaping stage, and returns the rows.

**The machinery exists** — this ticket wires it, it does not invent it:

- `PipeOp::Expand(PathRef)` parses (parser `ast.rs:177`) and **executes**
  (`engine/combine.rs:128` → `eval::expand`); Filter/Project/Extend/Limit/Sort/Distinct/
  Aggregate all execute the same way (`CombineOp` arms).
- The pushdown planner (`crates/pushdown`) lowers `PipeOp` → `CombineOp`.
- `rest_read_rows` (driver-http `lib.rs`) already performs the confined fetch + `DECODE`.
- Load-time structural confinement (`body_confined`) already exists in `declared_driver.rs`.

## Design (from the blueprint bullet — implement to this)

1. **Node resolution + `{param}` binding.** Match the requested path against the declared
   view-path template; bind `{param}` segments; substitute them into the body's wire source.
   (Slack needs the 0-param case; Chatwork's `/rooms/{room}/messages` is the shape to keep
   working — template match + substitution, tested hermetically.)
2. **Body parse + plan-time confinement.** Parse the stored body with `qfs_parser`. The body's
   source MUST be `/http/<self>/…` — reject anything else with a structured error (defense in
   depth over the load-time check). The body evaluator has NO resolver for any other mount, so
   a declared view structurally cannot read `/mail` (the anti-exfiltration invariant).
3. **Fetch + execute.** Fetch via the confined transport (`rest_read_rows`, honoring the
   body's `DECODE <codec>`), then execute the body's remaining ops through the engine
   (`PipeOp` → `CombineOp` → `qfs_engine`; `EXPAND messages` is the proving case). The user's
   outer residual (WHERE/LIMIT pushed into scan) composes AFTER the body pipeline.
4. **`OF`-type shaping.** After body evaluation, project to the declared type's columns and
   cast to its column types (the type is the delivered contract). `conformance()` on the
   result of a correct body now PASSES — flip the Slack-twin conformance expectation in the
   follow-up ticket (3/3), not here.
5. **Path decoupling falls out**: the view path is the mount address; the wire endpoint comes
   from the body source. `declared_mounts`/`declared_remap`/`RestReadDriver` re-wire from
   "outer path ⇒ wire resource" to "outer path ⇒ declared node ⇒ body ⇒ wire resource".
   DESCRIBE stays pure (no network) and reports the `OF` type's columns as the node schema.

## Key files

- `packages/qfs/crates/qfs/src/declared_driver.rs` — node resolution, body parse/confine,
  the body-evaluation entry
- `packages/qfs/crates/qfs/src/read_facets.rs` — `RestReadDriver::scan` calls body evaluation
- `packages/qfs/crates/qfs/src/describe.rs` — declared describe reports OF-type columns
- `packages/qfs/crates/pushdown`, `crates/engine` — reuse (no changes expected; if a lowering
  helper is missing, add it AS a public library fn, not a private fork)

## Resume knowledge (scouted 2026-07-05 on branch `work-20260705-173620` — start here)

The branch already carries the blueprint Tier-2 decision (commit on this branch) and ticket 1/3
(dotted cursor + redirect confinement) implemented; `declared_http_client` now takes `&DeclaredDriver`.
Seams verified by reading the code — do NOT re-scout:

- **The engine's only public execution entry** is `MiniEvaluator::execute(&PhysicalPlan,
  ScanResults) -> RowBatch` (`crates/engine/src/combine.rs`; re-exported in `engine/lib.rs`).
  The per-op fns (`eval::expand`, `filter`, `project`, …) are **`pub(crate)`** — reuse goes
  through `CombineOp`/`PhysicalPlan`, not by calling eval fns directly.
- **`CombineOp::Expand` executes** (`combine.rs:128` → `eval::expand`, eval.rs:476). The full
  op set: Filter/Project/ProjectExpr/Extend/Limit/Sort/Distinct/Aggregate/Expand/HashJoin/SetOp.
- **There is NO standalone public `lower_query`** — lowering lives inside
  `qfs_core::plan_query(stmt, &MountRegistry) -> PhysicalPlan` (`crates/core/src/plan.rs:85`).
  Two implementation options; prefer (a):
  (a) build a tiny **synthetic `MountRegistry`** containing exactly one mount for
  `/http/<name>` (profile: limit-only pushdown, like `/rest`) and call `plan_query` on the
  parsed body — full reuse of lowering + partition; the single ScanNode's fetch is our
  confined `rest_read_rows`; or (b) export a lowering helper from qfs-core as a new public fn
  (more invasive; only if (a) hits a wall).
- **The main read path to mirror** is `qfs_exec::execute_read` (`crates/exec/src/exec.rs:46`):
  plan_query → per-scan `ReadDriver::scan` → `MiniEvaluator::execute` → `apply_codecs`.
- **Trailing `DECODE`/`ENCODE` are dropped by pushdown** and applied post-engine by
  `crates/exec/src/codec.rs::apply_codecs`. In a body, the leading `DECODE json` is ALREADY
  performed by the wire fetch (`rest_read_rows` decodes via the RestApplier's codec, which
  yields the envelope row with object keys as columns) — so treat the body's `DECODE` as the
  codec DECLARATION consumed by the fetch, not a stage to re-execute.
- **The scan seam to replace** is `RestReadDriver::scan` (`crates/qfs/src/read_facets.rs:134`)
  — currently `rest_read_rows` + pushed-limit truncate, wrapped in `read_off_runtime` (keep
  that wrapper: live reqwest block_on must stay off the async runtime).
- Load-time structural confinement (`body_confined`/`json_paths_confined`) and
  `load_declared_types`/`conformance` already exist in `crates/qfs/src/declared_driver.rs`.
- The declared read/apply registration loops live in `crates/qfs/src/shell.rs:250` and
  `crates/qfs/src/commit.rs:327` (iterate `declared_mounts()`; `declared_remap` builds the
  `/rest/<name>` two-segment inner prefix via `MountRemap::new_prefixed`).
- Ticket-frontmatter validator: `type` ∈ enhancement|bugfix|refactoring|housekeeping,
  `effort` ∈ 0.1h…4h, `author` = email, `depends_on` entries end in `.md`.

## Quality gate (all hermetic, MockHttp)

- A declared view whose body is `/http/<self>/x |> DECODE json |> EXPAND messages` returns the
  UNWRAPPED message rows (not the envelope row).
- A parameterized view `/x/{id}/y` binds and substitutes into the wire path.
- The OF type shapes the result (columns + types match the declaration; conformance passes).
- A body naming a foreign source (`/http/other/…` or `/mail/...`) is rejected structurally at
  plan time with a secret-free error.
- A view mounted at a path unrelated to its wire endpoint (e.g. `/slack/history` over
  `conversations.history`) reads correctly — the decoupling proof.
- All existing gates green, sequential.
