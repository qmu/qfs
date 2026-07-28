---
type: Concern
concern_id: the-13-2-calibration-table-was
mission: [the-declared-slack-twin-retires-the-compiled-driver]
owner: a@qmu.jp
tickets: [20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md, 20260724014300-cf-queue-pull-twin-and-retirement.md]
origin_pr: 27
origin_pr_url: https://github.com/qmu/qfs/pull/27
origin_branch: work-20260724-011034
origin_commit: add5098
created_at: 2026-07-28T13:09:57+09:00
first_seen: 2026-07-28T13:09:57+09:00
last_seen: 2026-07-28T13:09:57+09:00
severity: moderate
status: active
resolved_by_pr:
resolved_by_commit:
---

# The §13.2 calibration table was mis-measured, and the first real conversion lands at the bar rather than under it

## Description

§13.2's "statements" column counted semicolons including the prose semicolons inside each script's comment header, so **every** pre-existing row was wrong (github_account 11→5, chatwork 15→10, cloudflare 22→16). Re-measuring also showed `slack_driver.qfs` at exactly 40 statement-lines — **at** the ~40 bar with zero headroom, against a projection of ~25–30 — so §13.2's claim that every projected twin lands under the bar does not survive its first test, and the CALL/write half is what the projections under-counted (see [3ac4508](https://github.com/qmu/qfs/commit/3ac4508) in `docs/blueprint.md`).

## How to Fix

Re-measure the github/drive/mail projections before their missions are scoped, treating the slack figure as the calibration point rather than the estimate; and decide whether the bar itself should move now that a real conversion has tested it.
