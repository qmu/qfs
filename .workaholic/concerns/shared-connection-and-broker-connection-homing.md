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
last_seen: 2026-07-16T16:14:56+09:00
severity: low
status: active
resolved_by_pr:
resolved_by_commit:
---

# shared_connection and broker_connection homing is the same question, deferred

## Description

The team-ownership registries (`shared_connection`, `broker_connection`) still live in the Project DB and are declarative by the same principle the re-homing established; the ticket records them as out of scope (M9 territory, own decision later) (see [ada28be](https://github.com/qmu/qfs/commit/ada28be))

## How to Fix

Decide their homing when the Managed Team work returns to them; the same migration + one-shot copy + reader-repoint pattern applies
