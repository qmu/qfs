---
created_at: 2026-06-25T09:47:50+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort:
commit_hash:
category:
depends_on: []
---

# git live execution (real RepoStore) + cloudflare (cf) status

## Overview

Two drivers whose live execution was deliberately deferred, grouped because each needs a backend
decision, not just wiring.

### git — needs a real `RepoStore`

`crates/driver-git/` has the real applier/compiler/path/relational logic, but per **ADR-0003**
`gix` was rejected on footprint/offline/wasm grounds and the object reader uses fixture output;
`RepoStore` (the COMMIT apply backing) is not backed by a real repository. `GitDriver::new(repos:
RepoResolver, applier: GitApplier)` → `git_apply_driver` is ready.

- **Build:** a production `RepoStore`/reader over a real git (options: shell out to the `git` CLI —
  zero new deps, the ADR-0003-friendly path; or a `gix`/pack backend — revisit the ADR). Confine to
  a binary-only leaf.
- **Wire:** register under DriverId `git` in `commit.rs` `live_registry()` + a planning mount;
  resolve repo paths (`/git/<repo>@<ref>/...`).
- **Verify:** genuinely E2E-verifiable here against a local temp git repo (offline) — a good first
  slice, unlike the networked drivers. Revisit ADR-0003 explicitly if adding a git dep.

### cf (cloudflare D1 / Workers) — parked, confirm status

The cf worker crate is parked offline (see ADR-0005 / t36 notes; the wasm/worker build is CI-only).
Action: confirm whether live cf execution is in scope at all, or remains parked. If in scope, it
rides the same `HttpExchange` transport pattern (cf has its own seam over `qfs-http-core`) for the
D1/REST surface; the Workers host stays CI-only. Until decided, keep it honestly documented as
parked (do not instruct it in docs/skill).

## Considerations

- ADR-0002/0003/0005 footprint + offline rules govern both backend choices.
- Patch bump + docs-in-lockstep per the umbrella ticket; do not document either as working until a
  live smoke passes.
