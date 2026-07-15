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
concern_id: cloudflare-declaration-design-remains-partial
severity: low
status: resolved
resolved_by_pr: b9e1137
resolved_by_commit: 
---

# Cloudflare declaration design remains partial

## Description

This branch uses explicit environment resource lists because the current `CREATE CONNECTION` shape cannot carry D1 database, KV namespace, and Queue handles cleanly yet (see [b9d2ad8](https://github.com/qmu/qfs/commit/b9d2ad8) in `packages/qfs/crates/qfs/src/cf.rs`).

## How to Fix

Design a per-resource Cloudflare declaration format, then migrate the environment-backed composition into declared connection state without losing fail-closed behavior.
