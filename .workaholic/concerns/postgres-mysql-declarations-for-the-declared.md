---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-28T12:55:39+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: postgres-mysql-declarations-for-the-declared
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# Postgres/MySQL declarations for the declared-registry path are partial

## Description

No `postgres.qfs` or `mysql.qfs` exists among the shipped declarations, and driver-sql has no comment coverage (see [3c6f995](https://github.com/qmu/qfs/commit/3c6f995)).

## How to Fix

Complete the Postgres/MySQL type and comment mappings on the declared-registry path.

