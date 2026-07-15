---
status: active
severity: low
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 
concern_id: bearer-gated-non-loopback-reconcile-round
origin_pr: 30
origin_pr_url: https://github.com/qmu/qfs/pull/30
origin_branch: work-20260707-180554
origin_commit: e7e44ee
mission:
---

# Bearer-gated (non-loopback) reconcile round is not live-verified

## Description

The recorded live provisioning verification ran under the loopback dev posture without the OAuth AS. The non-loopback path is covered only by the fail-closed unit test (a non-loopback bind without bearer material refuses the commit bridge); a full bearer-authenticated plan/apply round against a passphrase-booted daemon has not been run. (`bearer-gated-non-loopback-reconcile-round.md`, origin `e7e44ee`)

## How to Fix

Owner runs one bearer-gated round: boot with QFS_PASSPHRASE + System DB, obtain a token from the OAuth AS, and drive plan/apply against a non-loopback bind; record the result.

