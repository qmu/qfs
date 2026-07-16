---
type: Concern
concern_id: live-chatwork-behavior-change-awaits-owner
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260715190000-resume-development-in-the-new-public-repo.md, 20260716005029-unify-the-qfs-statement-splitter.md, 20260716120200-reinstall-replaces-a-declaration.md]
origin_pr: 1
origin_pr_url: https://github.com/qmu/qfs/pull/1
origin_branch: work-20260715-205333
origin_commit: ddb419e
created_at: 2026-07-16T15:16:32+09:00
first_seen: 2026-07-16T15:16:32+09:00
last_seen: 2026-07-16T16:14:56+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Live /chatwork behavior change awaits owner-attended verification

## Description

After [3bc2710](https://github.com/qmu/qfs/commit/3bc2710), /chatwork on this box resolves the newer view body (previously the oldest row won). Correct per the fix, but the live confirmation is owner-attended

## How to Fix

Owner runs a live /chatwork read post-merge and confirms the newer view contract is in effect

