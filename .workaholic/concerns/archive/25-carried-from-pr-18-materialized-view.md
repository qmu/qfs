---
type: Concern
mission: 
tickets: [20260706175249-multi-oauth-app-per-provider.md, 20260707013803-RESUME-v0026-shipped-queue-is-owner-gated.md, 20260707022409-ddl-event-log-schema.md, 20260707022410-record-ddl-events-on-config-writes.md, 20260707022411-dump-current-qfs-state.md, 20260707022412-restore-and-replay-qfs-state.md, 20260707034922-reorganize-qfs-docs-design-snapshot.md]
origin_pr: 25
origin_pr_url: https://github.com/qmu/qfs/pull/25
origin_branch: work-20260707-025845
origin_commit: 37bb365
created_at: 2026-07-07T04:35:44+09:00
last_seen: 2026-07-07T04:35:44+09:00
first_seen: 2026-07-07T04:35:44+09:00
concern_id: materialized-view-freshness-recording-is-not
severity: low
status: resolved
resolved_by_pr: b9d2ad8
resolved_by_commit: 
---

# (carried from PR #18) Materialized-view freshness recording is not wired

## Description

`last_run` is readable on `/server/views`, but refresh/materialization does not stamp it yet.

## How to Fix

Implement the queued materialized-view refresh ticket so refresh records `last_run` into the view config row.
