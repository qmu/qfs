---
type: Feedback
title: Policy-less or denied job re-fires every sweep
kind: concern
source: development
created_at: 2026-07-12T11:45:00+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: policy-less-or-denied-job-re
owner: 
mission: 
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
last_seen: 2026-07-28T13:09:57+09:00
---

# Policy-less or denied job re-fires every sweep

## Description

No sweeper source file appears in this branch's 49-file diff (see [c30fa0a](https://github.com/qmu/qfs/commit/c30fa0a)).

## How to Fix

Add back-off or quarantine semantics for denied jobs.
