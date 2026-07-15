---
status: active
severity: low
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 
concern_id: the-api-policy-row-gates-mcp
origin_pr: 30
origin_pr_url: https://github.com/qmu/qfs/pull/30
origin_branch: work-20260707-180554
origin_commit: e7e44ee
mission:
---

# The `api` policy row gates MCP, dashboard, and reconcile alike

## Description

The daemon's statement-bridge commit gate resolves the live `/server/policies` row named `api` (blueprint §16, Decision X). Because MCP, the dashboard bridge, and `qfs apply` share one executor, granting the `api` policy grants all three clients at once; absent the row the gate is the empty default-deny it always was. Codified, but worth operator awareness when widening the grant (a reconcile-gran… (`the-api-policy-row-gates-mcp.md`, origin `e7e44ee`)

## How to Fix

If per-client grants are ever needed, split the gate's policy resolution by client identity (bearer subject) rather than one shared row; until then document the shared-grant behavior in the operator guide.

