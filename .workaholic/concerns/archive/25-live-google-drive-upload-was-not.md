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
concern_id: live-google-drive-upload-was-not
severity: low
status: resolved
resolved_by_pr: 36
resolved_by_commit: (live rounds 2/4/5, branch work-20260712-114152)
---

# Live Google Drive upload was not re-run for the gdrive alias fix

## Description

The `/gdrive` fix is covered by hermetic mount, describe, and lazy apply-registry tests, but no live Google Drive upload was performed in this branch (see [d3d7888](https://github.com/qmu/qfs/commit/d3d7888) in `packages/qfs/crates/qfs/src/commit.rs`).

## How to Fix

When owner credentials are available, run a live `/gdrive` upload and read-back smoke test against a disposable Drive file, then record the result in a follow-up ticket or release note.

## Resolution

Live-proven on branch work-20260712-114152 (rounds 2, 4, 5): multi-row Drive INSERT, reply-with-attachment PDF into Drive, and a PDF→text→Drive pipeline — each uploaded and read back against a disposable Drive file. Recorded in release note work-20260712-114152.md.
