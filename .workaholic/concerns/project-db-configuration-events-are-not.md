---
type: Concern
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: [20260706175249-multi-oauth-app-per-provider.md, 20260707013803-RESUME-v0026-shipped-queue-is-owner-gated.md, 20260707022409-ddl-event-log-schema.md, 20260707022410-record-ddl-events-on-config-writes.md, 20260707022411-dump-current-qfs-state.md, 20260707022412-restore-and-replay-qfs-state.md, 20260707034922-reorganize-qfs-docs-design-snapshot.md]
origin_pr: 25
origin_pr_url: https://github.com/qmu/qfs/pull/25
origin_branch: work-20260707-025845
origin_commit: 37bb365
created_at: 2026-07-07T04:35:44+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-07T04:35:44+09:00
concern_id: project-db-configuration-events-are-not
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Project DB configuration events are not yet in the DDL event log

## Description

System DB-backed writes append DDL events transactionally, but Project DB-backed path/account state cannot share that transaction boundary yet (see [3385eb3](https://github.com/qmu/qfs/commit/3385eb3) in `packages/qfs/crates/qfs/src/sys.rs`). (`project-db-configuration-events-are-not.md`, origin `37bb365`)

## How to Fix

Add a Project DB event writer for `path_binding` and account/app consent mutations, with the same secret-redaction and hash-chain discipline, or introduce a cross-store event envelope that makes the two stores' boundaries explicit.

