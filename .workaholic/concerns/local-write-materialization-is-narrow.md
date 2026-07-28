---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-28T13:09:57+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: local-write-materialization-is-narrow
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
mission: 
---

# /local write materialization is narrow

## Description

driver-local was not modified here; this branch's write work was declared-CALL dispatch over `/http` (see [3c6f995](https://github.com/qmu/qfs/commit/3c6f995)).

## How to Fix

Widen the local write surface to carry a multi-column payload.

