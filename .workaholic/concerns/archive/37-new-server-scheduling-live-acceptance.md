---
type: Concern
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
tickets: []
origin_pr: 37
origin_pr_url: https://github.com/qmu/qfs/pull/37
origin_branch: work-20260713-150833
origin_commit: 14b9a41
created_at: 2026-07-13T16:45:00+09:00
last_seen: 2026-07-13T16:45:00+09:00
first_seen: 2026-07-13T16:45:00+09:00
concern_id: server-scheduling-live-acceptance-still-owed
severity: low
status: resolved
resolved_by_pr: 84a224f
resolved_by_commit: 
---

> **Resolved** 2026-07-13 (branch `work-20260713-185925`, owner-attended): round 9 re-run on
> `qfs 0.0.61` PASSED — `qfs serve` sweeper fired a 1m `/local` upsert JOB in <1s
> (`outcome=fired affected=1`), the tick file was written, the durable `last_run` survived a restart
> with no spurious re-fire, and the restarted daemon resumed the schedule. Full evidence on the
> archived sweeper ticket `20260713130000-sweeper-job-body-format-mismatch.md`; mission
> server-scheduling acceptance ticked. `resolved_by_commit`/`_pr` filled at report/ship time.

# Server-scheduling live acceptance still owed — re-run live round 9

## Description

The v0.0.60 sweeper fix (fire_one rehydrates the canonical PlanSpec) is hermetically proven — a live-committer test writes a real tick file — but the mission's server-scheduling acceptance needs the owner-attended live re-run: `qfs serve` fires a 1-minute JOB within 90s, the runs ledger shows `fired` with `affected 1`, and history survives restart. Code landed; live proof pending.

## How to Fix

Owner runs round 9 in a dedicated session against v0.0.60 and records the evidence on the archived sweeper ticket; tick the mission server-scheduling acceptance.
