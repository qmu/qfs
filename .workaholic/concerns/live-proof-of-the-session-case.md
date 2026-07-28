---
type: Concern
concern_id: live-proof-of-the-session-case
mission: [a-request-resolves-to-a-principal-the-query-path-can-read]
owner: a@qmu.jp
tickets: [20260719101201-identity-read-back-tells-the-truth.md, 20260719101202-thread-the-request-principal-to-the-scan-seam.md, 20260719101203-role-stays-not-a-grant-and-the-open-decision-stays-open.md, 20260719101204-one-live-round-developer-attended.md, 20260723090000-serve-sys-and-session-principal-resolution.md]
origin_pr: 23
origin_pr_url: https://github.com/qmu/qfs/pull/23
origin_branch: work-20260719-101118
origin_commit: 9241270
created_at: 2026-07-24T01:02:01+09:00
first_seen: 2026-07-24T01:02:01+09:00
last_seen: 2026-07-28T12:51:29+09:00
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Live proof of the session case deferred on host disk

## Description

Blocked on host disk, which no code change on this branch clears; the container re-run is still owed (see [9241270](https://github.com/qmu/qfs/commit/9241270)).

## How to Fix

Reclaim disk and re-run the containerised live round.

