---
type: Concern
concern_id: the-statement-bridge-s-read-leg
mission: [what-a-principal-can-see-and-do-is-granted-by-policy]
owner: a@qmu.jp
tickets: [20260724013000-enforce-policy-on-the-serve-read-path.md, 20260724013100-every-grant-axis-bites-on-reads.md, 20260724013200-fail-closed-and-structured-denial-on-reads.md]
origin_pr: 25
origin_pr_url: https://github.com/qmu/qfs/pull/25
origin_branch: work-20260724-011032
origin_commit: 3ef8fb7
created_at: 2026-07-28T12:51:29+09:00
first_seen: 2026-07-28T12:51:29+09:00
last_seen: 2026-07-28T12:51:29+09:00
severity: moderate
status: active
resolved_by_pr:
resolved_by_commit:
---

# The statement bridge's read leg is ungated

## Description

`POST /api/run` with `{"mode":"read"}` parses a caller-supplied statement and runs it through `qfs_exec::block_on_read` with no policy resolution and a hardcoded `RequestContext::anonymous()` — strictly wider than the endpoint face this branch just closed, so a caller refused by an endpoint policy can read the same path through the bridge (see [4ce511e](https://github.com/qmu/qfs/commit/4ce511e) in `crates/qfs/src/dashboard.rs` and `mcp.rs`). Pre-existing and not a regression, mitigated only by the loopback-only default bind — which is a deployment posture, not an authorization control. Ticketed at `.workaholic/tickets/todo/a-qmu-jp/20260725110500-gate-the-statement-bridge-read-leg.md`.

## How to Fix

Decide which policy governs the bridge — a named bridge policy row, a setting, or the operator-local super-admin posture — then resolve and enforce it on the read leg the way the endpoint face now does.
