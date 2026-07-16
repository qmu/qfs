---
type: Concern
concern_id: the-operator-s-live-box-runs
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

# The operator's live box runs the one-shot copy on first post-upgrade boot

## Description

The live registry (real bindings and consents) sits in the legacy `project.db` on the operator's box; the first boot of a binary containing [ada28be](https://github.com/qmu/qfs/commit/ada28be) performs the copy into the System DB. The copy fails the DB open loudly if it cannot complete, so a silently-empty registry cannot slip through — but the confirmation read is owner-attended

## How to Fix

After upgrading, the owner runs `qfs connect --list` (or a /chatwork read) and confirms the live mounts carried across; the COPY event is visible in the DDL event log
