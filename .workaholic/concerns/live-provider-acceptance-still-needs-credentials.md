---
type: Concern
mission: 
tickets: [20260630203090-cf-live-d1-kv-queue.md, 20260706120400-materialized-view-refresh-last-run.md, 20260706183441-postgres-value-round-trips.md, 20260707043312-drive-blob-upload-report-copy.md]
origin_pr: 26
origin_pr_url: https://github.com/qmu/qfs/pull/26
origin_branch: work-20260707-045409
origin_commit: d8442ef
created_at: 2026-07-07T05:42:51+09:00
last_seen: 2026-07-16T16:14:56+09:00
first_seen: 2026-07-07T05:42:51+09:00
concern_id: live-provider-acceptance-still-needs-credentials
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Live provider acceptance still needs credentials

## Description

Cloudflare/Postgres/Drive live acceptance still needs owner credentials unavailable in-container; cf.rs/sql_backends.rs/session.rs unchanged on this branch

## How to Fix

Run the live provider acceptance rounds in an owner-attended session with credentials

