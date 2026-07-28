---
type: Concern
mission: 
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
created_at: 2026-07-12T11:45:00+09:00
last_seen: 2026-07-28T12:51:29+09:00
first_seen: 2026-07-12T11:45:00+09:00
concern_id: policy-less-or-denied-job-re
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Policy-less or denied job re-fires every sweep

## Description

Re-graded from low this run. `sweeper.rs` still has no back-off for denied jobs (see [c30fa0a](https://github.com/qmu/qfs/commit/c30fa0a)). This branch materially raises the odds of hitting it: enabling `SELECT` enforcement is a declared hard break in which every previously-passing policy-less read now denies, so any scheduled job whose query is a pure read becomes a permanently-denied job that re-fires on every sweep after upgrade.

## How to Fix

Add back-off or quarantine semantics for denied jobs, and surface policy-less scheduled reads at upgrade so an operator can attach a policy before the first sweep.

