---
type: Concern
concern_id: the-dead-project-db-config-tables
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260716143641-rehome-declarative-tables-into-the-system-db.md, 20260716144816-RESUME-report-and-ship-work-20260715-205333.md]
origin_pr: 2
origin_pr_url: https://github.com/qmu/qfs/pull/2
origin_branch: work-20260716-152000
origin_commit: 974c72d
created_at: 2026-07-16T16:14:56+09:00
first_seen: 2026-07-16T16:14:56+09:00
last_seen: 2026-07-28T12:51:29+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# The dead Project-DB config tables await their drop migration

## Description

The schema still carries `project_path_bindings.sql`, and neither `path_binding` nor `connection_consent` is dropped (see [974c72d](https://github.com/qmu/qfs/commit/974c72d)). The sequencing precondition is a deployment event this branch does not supply.

## How to Fix

File the drop migration once a release containing the boot copy has booted live.

