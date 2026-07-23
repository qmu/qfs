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
last_seen: 2026-07-24T01:02:01+09:00
severity: moderate
status: active
resolved_by_pr:
resolved_by_commit:
---

# Live proof of the session case deferred on host disk

## Description

Mission acceptance item 8's in-container live proof of the session-carrying case shipped its code dependencies (see [b4f1997](https://github.com/qmu/qfs/commit/b4f1997)) and is proven hermetically, but the container re-run needs ~13G free on `/` and the shared host had ~6.9G; per the ticket's host-safety rule the round did not gamble on the disk (see the addendum in ticket 20260719101204).

## How to Fix

Re-run `containers/live-round/run.sh` once `/` has ~13G free, paste the fresh transcript into the ticket, and tick the live-round leg. Resource contention, not an implementation gap.
