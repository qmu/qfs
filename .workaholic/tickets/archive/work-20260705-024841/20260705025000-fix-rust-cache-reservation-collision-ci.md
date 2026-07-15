---
created_at: 2026-07-05T02:50:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 0.25h
commit_hash: 41fd0c8
category: Changed
depends_on: []
---

# CI/release: parallel jobs collide on one rust-cache key → recurring "Unable to reserve cache" error

## Problem (owner-reported: annoying recurring CI error)

Every CI and release run logs a red error on the `Post Run Swatinem/rust-cache@v2` step:

```
Failed to save: Unable to reserve cache with key v0-rust-build-Linux-x64-…-bea33e0c, another job
may be creating this cache.
Failed to save: Unable to reserve cache with key v0-rust-build-Darwin-arm64-…, another job may …
```

The run **passes** (the cache from the first job is still usable), but the red "Failed to save" line
looks like a failure and shows up on every run.

## Root cause

`Swatinem/rust-cache@v2` derives its key from the **host** arch + the `Cargo.lock` hash — NOT the
job or the build target. Several jobs run in parallel on the same host and therefore reserve the
SAME key:

- `ci.yml`: `clippy`, `build-test`, `cross-*` (×2 targets), `wasm32`, and `release-artifacts` all run
  on `ubuntu-latest` (`Linux-x64`).
- `release.yml`: the two macOS targets share the `arm64` macOS host; the two `musl` targets share the
  `x64` ubuntu host.

The first job to finish reserves the key; the second's save is rejected with "Unable to reserve cache".

## Fix

Give each rust-cache invocation a distinct `key:` so no two parallel jobs share a cache key (each
keeps its own cache — no reservation contention):

- `ci.yml`: `key: clippy` / `build-test` / `cross-${{ matrix.target }}` / `wasm32` / `release-artifacts`.
- `release.yml`: `key: ${{ matrix.target }}`.

## Quality Gate

- A CI run on the PR no longer logs "Unable to reserve cache … another job may be creating this
  cache" on any `Post Run rust-cache` step; all jobs stay green.
- No behaviour change to the built binary — this is CI/infra only, so it merges without a version
  bump or a new release (the deliverable binary is unchanged).

## Key files

- `.github/workflows/ci.yml`, `.github/workflows/release.yml`
