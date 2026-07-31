---
type: Feedback
title: The dead Project-DB config tables await their drop migration
kind: concern
source: development
created_at: 2026-07-16T16:14:56+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: the-dead-project-db-config-tables
owner: 
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260716143641-rehome-declarative-tables-into-the-system-db.md, 20260716144816-RESUME-report-and-ship-work-20260715-205333.md]
origin_pr: 2
origin_pr_url: https://github.com/qmu/qfs/pull/2
origin_branch: work-20260716-152000
origin_commit: 974c72d
last_seen: 2026-07-28T13:09:57+09:00
---

# The dead Project-DB config tables await their drop migration

## Description

Both dead tables are still in the schema with no drop migration (see [974c72d](https://github.com/qmu/qfs/commit/974c72d)).

## How to Fix

File the drop migration once a release containing the boot copy has booted live.
