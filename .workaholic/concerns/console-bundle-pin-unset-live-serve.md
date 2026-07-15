---
type: Concern
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
created_at: 2026-07-05T01:25:53+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-05T01:25:53+09:00
concern_id: console-bundle-pin-unset-live-serve
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
mission:
---

# Console bundle pin unset; live serve + release stamp pending the plgg bundle

## Description

The console delivery machinery is complete and tested, but `PINNED_BUNDLE` is empty (`console-bundle-pin-unset-live-serve.md`, origin `72c8950`)

## How to Fix

When the plgg bundle publishes, stamp its URL+hash into `PINNED_BUNDLE`, wire the real

