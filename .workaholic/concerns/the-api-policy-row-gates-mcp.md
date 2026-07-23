---
status: active
severity: low
last_seen: 2026-07-23T23:59:51+09:00
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

The single 'api' policy row still grants MCP, dashboard and reconcile alike; no per-client gate split was made on this branch

## How to Fix

Split the api policy row into per-client gates if the access-control review requires it

