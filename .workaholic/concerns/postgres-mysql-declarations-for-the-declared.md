---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: postgres-mysql-declarations-for-the-declared
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Postgres/MySQL declarations for the declared-registry path are partial

## Description

Live Postgres/MySQL `/sql` backends work when configured ([ca67fb8]), but from the CREATE CONNECTION declared-registry path the binary's declared `/sql` was historically SQLite-only, and `sql`/`git` still ride the declared-connection seam rather than the new `path_binding` registry (documented CONNECT-epic follow-up). NUMERIC/TIMESTAMP/UUID/JSON column round-trips and `--` comments in `connections… (`postgres-mysql-declarations-for-the-declared.md`, origin `3c6f995`)

## How to Fix

Move `sql`/`git` onto `path_binding`, broaden column-type coverage, and add comment support to the connections parser.

