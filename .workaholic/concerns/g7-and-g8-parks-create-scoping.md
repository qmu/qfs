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
last_seen: 2026-07-24T00:40:59+09:00
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# G7 and G8 parks create scoping risk for downstream conversions

## Description

Blob-archetype ergonomics (G7) and non-REST-arm handling (G8) are recorded as named parks without trigger conditions (see [fb83df5](https://github.com/qmu/qfs/commit/fb83df5)). The conversion playbook depends on their status being clear to incoming sessions; deferred work risks being silently re-discovered or forgotten.

## How to Fix

Record concrete follow-up mission names and trigger conditions for G7 and G8 in blueprint §13.3 (not just "parked"), so a fresh session can detect their absence and either scope around them or escalate if they become load-bearing.
