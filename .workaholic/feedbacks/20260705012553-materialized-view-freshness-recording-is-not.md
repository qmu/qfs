---
type: Feedback
title: Materialized-view freshness recording is not wired
kind: concern
source: development
created_at: 2026-07-05T01:25:53+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: materialized-view-freshness-recording-is-not
owner: 
mission: 
tickets: []
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
last_seen: 2026-07-05T01:25:53+09:00
closed: resolved
resolved_by_pr: b9d2ad8
---

# Materialized-view freshness recording is not wired

## Description

`last_run` is a readable column on `/server/views` (honest `null`), but nothing yet

## How to Fix

Have the materialize/refresh step stamp `last_run` into the view's config row (the same
