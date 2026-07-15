---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: extend-on-the-read-path-is
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
mission:
---

# EXTEND on the read path is now a real operation (behaviour change)

## Description

EXTEND was previously a silent no-op on reads; it now actually computes per-row values ([b5a4eec]). This is a correctness fix but a behaviour change — any pipeline that (accidentally) relied on the old no-op now behaves differently, and the array/struct literal forms became expression constructors (an experimental hard break). (`extend-on-the-read-path-is.md`, origin `3c6f995`)

## How to Fix

Audit cookbook/tests for EXTEND uses (suite is green, no regressions caught) and note the change prominently in the release note so downstream scripts expecting the old behaviour are updated.

