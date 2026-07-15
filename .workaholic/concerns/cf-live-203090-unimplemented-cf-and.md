---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: cf-live-203090-unimplemented-cf-and
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# /cf live (203090) unimplemented; /cf and /rest are placeholder mounts

## Description

`/cf` and `/rest` are reachable, cred-free planning/describe mounts ([8cce093]), but live credentialed read/commit and per-resource config (which D1/KV/queues; which REST resource maps) are follow-ups needing a richer connection declaration; `/cf` live verification needs the owner's CF token, so 203090 is deferred. (`cf-live-203090-unimplemented-cf-and.md`, origin `3c6f995`)

## How to Fix

Design a per-resource connection declaration beyond the current (driver, locator, secret) shape, then wire read/apply facets and live-verify with the owner's token; roadmap already reflects this as deferred.

