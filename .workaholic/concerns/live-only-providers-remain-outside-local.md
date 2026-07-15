---
type: Concern
mission:
tickets: [20260706175249-multi-oauth-app-per-provider.md, 20260707013803-RESUME-v0026-shipped-queue-is-owner-gated.md, 20260707022409-ddl-event-log-schema.md, 20260707022410-record-ddl-events-on-config-writes.md, 20260707022411-dump-current-qfs-state.md, 20260707022412-restore-and-replay-qfs-state.md, 20260707034922-reorganize-qfs-docs-design-snapshot.md]
origin_pr: 25
origin_pr_url: https://github.com/qmu/qfs/pull/25
origin_branch: work-20260707-025845
origin_commit: 37bb365
created_at: 2026-07-07T04:35:44+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-07T04:35:44+09:00
concern_id: live-only-providers-remain-outside-local
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Live-only providers remain outside local proof

## Description

The design snapshot intentionally documents live-only gates for external providers, but local tests still prove only parser, preview, registry, and hermetic mock behavior for those services (see [e8c0d82](https://github.com/qmu/qfs/commit/e8c0d82) in `docs/guide/design-snapshot.md`). (`live-only-providers-remain-outside-local.md`, origin `37bb365`)

## How to Fix

Keep owner-live acceptance tickets for provider-specific paths such as Cloudflare, Postgres, and Google Drive, and record each credentialed verification separately from the hermetic release gate.

