---
type: Concern
mission: 
tickets: [20260630203090-cf-live-d1-kv-queue.md, 20260706120400-materialized-view-refresh-last-run.md, 20260706183441-postgres-value-round-trips.md, 20260707043312-drive-blob-upload-report-copy.md]
origin_pr: 26
origin_pr_url: https://github.com/qmu/qfs/pull/26
origin_branch: work-20260707-045409
origin_commit: d8442ef
created_at: 2026-07-07T05:42:51+09:00
last_seen: 2026-07-07T05:42:51+09:00
first_seen: 2026-07-07T05:42:51+09:00
concern_id: local-rust-verification-remains-unavailable-in
severity: low
status: resolved
resolved_by_pr: 33
resolved_by_commit: 
---

# Local Rust verification remains unavailable in this container

## Description

`cargo`, `rustfmt`, and `rustup` are still not installed in this container, so local Rust verification cannot be reproduced here. GitHub Actions run [`28821011194`](https://github.com/qmu/qfs/actions/runs/28821011194) passed the required release gates that are available in CI: `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build -p qfs-host --target wasm32-unknown-unknown`, and x86_64/aarch64 cross-compiles.

## How to Fix

Install the Rust toolchain in the agent container if local reproduction is required; otherwise use GitHub Actions as the release gate for this branch.

## Resolution (2026-07-12, PR #33)

Obsolete by environment change: the current development host has the full Rust toolchain, and every
release gate on branch `work-20260711-121525` ran locally (`cargo test --workspace`, clippy
`-D warnings`, fmt, gen-docs/gen-skills, check-migrations at `a59f914`). No code fix was involved.
