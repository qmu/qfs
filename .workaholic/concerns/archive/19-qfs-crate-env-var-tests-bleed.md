---
type: Concern
origin_pr: 19
origin_pr_url: https://github.com/qmu/qfs/pull/19
origin_branch: work-20260705-015204
origin_commit: 1347064
created_at: 2026-07-05T02:27:06+09:00
last_seen: 2026-07-05T02:27:06+09:00
first_seen: 2026-07-05T02:27:06+09:00
concern_id: qfs-crate-env-var-tests-bleed
severity: low
status: resolved
resolved_by_pr: 949f767
resolved_by_commit: 
---

# qfs-crate env-var tests bleed `XDG_CONFIG_HOME` under parallel `cargo test` (crash fixed; hygiene remains)

## Description

The `qfs`-crate `init`/`oauth`/`store` tests mutate process-global env and, under

## How to Fix

Ticket `20260705022000` (filed this branch): route the env-mutating tests through a
