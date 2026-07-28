---
type: Concern
mission: 
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
created_at: 2026-07-12T11:45:00+09:00
last_seen: 2026-07-28T12:55:39+09:00
first_seen: 2026-07-12T11:45:00+09:00
concern_id: policy-less-or-denied-job-re
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Policy-less or denied job re-fires every sweep

## Description

`sweeper.rs` still resolves a policy-less or dangling reference to a deny and records `CronOutcome::Denied` per firing with no suppression of the next sweep (see [c30fa0a](https://github.com/qmu/qfs/commit/c30fa0a)).

## How to Fix

Add back-off or quarantine semantics for denied jobs.

