---
type: Concern
concern_id: append-era-duplicate-rows-persist-on
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

# Append-era duplicate rows persist on disk but resolve correctly

## Description

After [3bc2710](https://github.com/qmu/qfs/commit/3bc2710), newest-per-key reads heal the operator's 14 append-era duplicate rows without re-install, but the rows remain physically on disk. Compacting them needs an uninstall surface (a deliberate non-goal of this branch)

## How to Fix

Implement a bundle-aware uninstall surface that removes superseded rows

