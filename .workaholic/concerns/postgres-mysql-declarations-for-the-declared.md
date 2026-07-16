---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-16T15:16:32+09:00
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

sql/git still ride the declared-connection seam rather than path_binding, and column-type/comment coverage is unchanged; branch did not touch the SQL backends or connections parser body

## How to Fix

Complete Postgres/MySQL declarations with full column-type and comment coverage (ruled to wait behind the re-homing ticket)

