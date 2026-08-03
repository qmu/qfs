---
type: Feedback
title: Postgres/MySQL declarations for the declared-registry path are partial
kind: concern
source: development
created_at: 2026-07-02T01:21:00+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: postgres-mysql-declarations-for-the-declared
owner: 
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: []
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
last_seen: 2026-07-28T13:09:57+09:00
---

# Postgres/MySQL declarations for the declared-registry path are partial

## Description

The re-homing blocker cleared, but one `sql.qfs` example exists and no per-engine Postgres/MySQL declaration (see [3c6f995](https://github.com/qmu/qfs/commit/3c6f995)). Now actionable rather than parked.

## How to Fix

Complete the Postgres/MySQL type and comment mappings on the declared-registry path.


## Re-grade (2026-07-28T21:48:57+09:00)

- severity: low -> moderate
- rationale: The blocker this concern was parked behind has cleared: commit 23991d5 made path_binding the only sql/git source, retiring the declared-connection seam half. The remaining work - per-engine Postgres and MySQL declarations with full column-type and comment coverage - is now actionable rather than waiting, and low would drop it below the promotion floor just as it becomes doable.
