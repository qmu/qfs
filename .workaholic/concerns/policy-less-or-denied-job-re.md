---
type: Concern
mission:
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
created_at: 2026-07-12T11:45:00+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-12T11:45:00+09:00
concern_id: policy-less-or-denied-job-re
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Policy-less or denied job re-fires every sweep

## Description

A policy-less or denied job re-fires every sweep by the ruled not-stamped semantics — visible denied runs, history capped at 50 (see 4de2f42 in `packages/qfs/crates/qfs/src/sweeper.rs`). (`policy-less-or-denied-job-re.md`, origin `c30fa0a`)

## How to Fix

Review the sweep retry semantics after live operation; consider a denied-job backoff if the visible denied-run churn proves noisy.

