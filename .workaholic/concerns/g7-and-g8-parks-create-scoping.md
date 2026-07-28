---
type: Concern
concern_id: g7-and-g8-parks-create-scoping
mission: [the-declared-driver-dsl-covers-the-compiled-drivers-concisely]
owner: a@qmu.jp
tickets: [20260722091100-coverage-inventory-of-compiled-driver-surfaces.md, 20260722091200-rule-the-semantic-gaps-in-blueprint-13.md, 20260722091300-ship-read-over-post-hermetically.md, 20260722091400-conciseness-bar-stated-and-measured.md, 20260722091500-conversion-playbook-and-honest-tiering.md]
origin_pr: 21
origin_pr_url: https://github.com/qmu/qfs/pull/21
origin_branch: work-20260722-084646
origin_commit: f52592b
created_at: 2026-07-24T00:40:59+09:00
first_seen: 2026-07-24T00:40:59+09:00
last_seen: 2026-07-28T12:51:29+09:00
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# G7 and G8 parks create scoping risk for downstream conversions

## Description

G7 and G8 are still parks in the blueprint with no follow-up mission names and no trigger conditions, so a fresh session cannot detect their absence (see [f52592b](https://github.com/qmu/qfs/commit/f52592b)).

## How to Fix

Name the follow-up mission for each park, or record the trigger that reopens it.

