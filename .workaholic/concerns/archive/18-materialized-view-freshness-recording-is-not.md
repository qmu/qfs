---
type: Concern
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
created_at: 2026-07-05T01:25:53+09:00
last_seen: 2026-07-05T01:25:53+09:00
first_seen: 2026-07-05T01:25:53+09:00
concern_id: materialized-view-freshness-recording-is-not
severity: low
status: resolved
resolved_by_pr: b9d2ad8
resolved_by_commit: 
---

# Materialized-view freshness recording is not wired

## Description

`last_run` is a readable column on `/server/views` (honest `null`), but nothing yet

## How to Fix

Have the materialize/refresh step stamp `last_run` into the view's config row (the same
