---
type: Concern
mission:
tickets: [20260630203090-cf-live-d1-kv-queue.md, 20260706120400-materialized-view-refresh-last-run.md, 20260706183441-postgres-value-round-trips.md, 20260707043312-drive-blob-upload-report-copy.md]
origin_pr: 26
origin_pr_url: https://github.com/qmu/qfs/pull/26
origin_branch: work-20260707-045409
origin_commit: d8442ef
created_at: 2026-07-07T05:42:51+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-07T05:42:51+09:00
concern_id: live-provider-acceptance-still-needs-credentials
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Live provider acceptance still needs credentials

## Description

Cloudflare, Postgres, and Google Drive behavior is wired but not live-verified in this container because the required provider credentials and live resources were not available (see [b9d2ad8](https://github.com/qmu/qfs/commit/b9d2ad8) in `packages/qfs/crates/qfs/src/cf.rs`, `packages/qfs/crates/qfs/src/sql_backends.rs`, and `packages/qfs/crates/exec/src/shell/session.rs`). (`live-provider-acceptance-still-needs-credentials.md`, origin `d8442ef`)

## How to Fix

Run the live Cloudflare D1/KV/Queue smoke tests with `CF_ACCOUNT_ID`/`CF_API_TOKEN`, a live Postgres `SELECT` over NUMERIC/timestamp/UUID/JSON columns, and a disposable Drive `cp /local/... /drive/...` upload/read-back check.

