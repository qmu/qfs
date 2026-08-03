---
type: Feedback
title: /sql connection registries are split-brain (run vs describe)
kind: concern
source: development
created_at: 2026-07-05T01:25:53+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: sql-connection-registries-are-split-brain
owner: 
mission: 
tickets: []
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
last_seen: 2026-07-05T01:25:53+09:00
closed: resolved
resolved_by_pr: f67ef53
---

# /sql connection registries are split-brain (run vs describe)

## Description

Verifying the SQLite DBMS surface found that `qfs run` builds the `sql` driver from

## How to Fix

Unify the connection source of truth so any declared `/sql` connection feeds both the
