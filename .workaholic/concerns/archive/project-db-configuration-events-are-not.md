---
type: Concern
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: [20260706175249-multi-oauth-app-per-provider.md, 20260707013803-RESUME-v0026-shipped-queue-is-owner-gated.md, 20260707022409-ddl-event-log-schema.md, 20260707022410-record-ddl-events-on-config-writes.md, 20260707022411-dump-current-qfs-state.md, 20260707022412-restore-and-replay-qfs-state.md, 20260707034922-reorganize-qfs-docs-design-snapshot.md]
origin_pr: 25
origin_pr_url: https://github.com/qmu/qfs/pull/25
origin_branch: work-20260707-025845
origin_commit: 37bb365
created_at: 2026-07-07T04:35:44+09:00
last_seen: 2026-07-16T15:16:32+09:00
first_seen: 2026-07-07T04:35:44+09:00
concern_id: project-db-configuration-events-are-not
severity: moderate
status: resolved
resolved_by_pr: 2
resolved_by_commit: ada28be
---

# Project DB configuration events are not yet in the DDL event log

## Description

Judged against the owner's choice-C ruling, not this concern's own How-to-Fix: the fix is now to re-home path_binding + connection_consent into the System DB (Project DB becomes the vault proper) so config writes share the insert_driver-style ledger transaction, superseding the cross-store-envelope suggestion. Ticket 20260716143641 is in todo/ and unimplemented — sys.rs path_binding/connection_consent writes still emit only a best-effort post-commit AuditEvent and no DdlEvent

## How to Fix

Implement ticket 20260716143641 (re-home the declarative tables into the System DB) on its own branch

