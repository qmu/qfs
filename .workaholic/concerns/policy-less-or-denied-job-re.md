---
type: Concern
mission: 
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
created_at: 2026-07-12T11:45:00+09:00
last_seen: 2026-07-24T00:48:25+09:00
first_seen: 2026-07-12T11:45:00+09:00
concern_id: policy-less-or-denied-job-re
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Policy-less or denied job re-fires every sweep

## Description

Sweeper denied/policy-less re-fire semantics remain as-is pending live operation; sweeper.rs was not modified on this branch

## How to Fix

Review and adjust sweeper re-fire semantics based on live operational experience

