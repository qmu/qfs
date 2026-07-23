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
last_seen: 2026-07-23T23:59:51+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# The dead Project-DB config tables await their drop migration

## Description

`path_binding` and `connection_consent` remain physically present (but dead) in the Project DB after [ada28be](https://github.com/qmu/qfs/commit/ada28be) — deliberately: the drop is a later Project-DB migration that must not be able to run before a release containing the boot copy has shipped (data-safety sequencing, not a compatibility period)

## How to Fix

After this release ships and the operator's live box has booted the copy, file the Project-DB migration that drops both dead tables

