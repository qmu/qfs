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
concern_id: drive-single-file-select-content-still
severity: low
status: resolved
resolved_by_pr: ef393b9
resolved_by_commit: 
---

> **Resolved** on branch `work-20260713-185925` (ticket
> `20260713191608-gdrive-content-schema-unification.md`): both gdrive read paths converge on
> `FileMeta::content_schema()` (11 listing columns + nullable `content`); `describe()` advertises it,
> a single-file read materialises the bytes, a folder listing carries a null `content`. So
> `/drive/<file> |> select content |> transform` type-checks at plan time and the round-5 struct
> bypass is no longer needed. `resolved_by_commit`/`_pr` filled at report/ship time.

# /drive single-file select content still diverges (gdrive file vs folder schema)

## Description

The v0.0.60 `/local` schema-widen (describe advertises a nullable `content` column) does not extend to gdrive: a gdrive file-content read returns `name/mime_type/size/md5/content`, a DIFFERENT column set from the folder listing (`id/name/mime_type/parents/…`). Advertising `content` in gdrive describe needs the file-vs-folder read schema unified first. Until then, `/drive/<file> |> select content` fails plan-time type-check and the round-5 struct bypass remains the workaround.

## How to Fix

Unify the gdrive file-content and folder-listing describe schemas (e.g. a superset with nullable `content`), mirroring the /local fix, then re-point any drive extraction recipe off the struct bypass.
