---
type: Concern
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
tickets: []
origin_pr: 37
origin_pr_url: https://github.com/qmu/qfs/pull/37
origin_branch: work-20260713-150833
origin_commit: 14b9a41
created_at: 2026-07-13T16:45:00+09:00
last_seen: 2026-07-13T16:45:00+09:00
first_seen: 2026-07-13T16:45:00+09:00
concern_id: driver-fs-shares-the-local-content
severity: low
status: resolved
resolved_by_pr: 433788f
resolved_by_commit: 
---

> **Resolved** on branch `work-20260713-185925` (ticket
> `20260713190432-driver-fs-content-column-parity.md`): `FsRow::content_schema()` added, `describe()`
> advertises the nullable `content` column, and a single-file `/fs` read materialises the bytes via
> `fs_core::read_blob` — mirroring the `/local` v0.0.60 fix. `resolved_by_commit`/`_pr` filled at
> report/ship time.

# driver-fs shares the /local content-omission

## Description

The `QFS_FS_<NAME>` named-roots driver (`driver-fs`) has the identical `FsRow` metadata-only describe schema `/local` had before v0.0.60, so `/fs/<file> |> select content |> transform` fails the same plan-time UnknownColumn. Not exercised by the live rounds (round 5 used /local), so scoped out of this branch.

## How to Fix

Apply the same widen as the /local fix: FsRow describe advertises a nullable `content` column, and directory/glob listings carry a null `content` so plan and runtime schemas agree.
