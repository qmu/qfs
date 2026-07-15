---
type: Concern
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
created_at: 2026-07-05T01:25:53+09:00
last_seen: 2026-07-05T01:25:53+09:00
first_seen: 2026-07-05T01:25:53+09:00
concern_id: sql-connection-registries-are-split-brain
severity: moderate
status: resolved
resolved_by_pr: f67ef53
resolved_by_commit: 
---

# /sql connection registries are split-brain (run vs describe)

## Description

Verifying the SQLite DBMS surface found that `qfs run` builds the `sql` driver from

## How to Fix

Unify the connection source of truth so any declared `/sql` connection feeds both the
