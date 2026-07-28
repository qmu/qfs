---
type: Concern
concern_id: shared-connection-and-broker-connection-homing
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260716143641-rehome-declarative-tables-into-the-system-db.md, 20260716144816-RESUME-report-and-ship-work-20260715-205333.md]
origin_pr: 2
origin_pr_url: https://github.com/qmu/qfs/pull/2
origin_branch: work-20260716-152000
origin_commit: 974c72d
created_at: 2026-07-16T16:14:56+09:00
first_seen: 2026-07-16T16:14:56+09:00
last_seen: 2026-07-28T12:55:39+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# shared_connection and broker_connection homing is the same question, deferred

## Description

Both registries are still defined in the Project DB schema with no re-homing migration; M9 territory (see [974c72d](https://github.com/qmu/qfs/commit/974c72d)).

## How to Fix

Schedule the M9 work, or rule the homing strategy separately.

