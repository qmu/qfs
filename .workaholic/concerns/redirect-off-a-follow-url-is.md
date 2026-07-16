---
type: Concern
mission: 
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
created_at: 2026-07-12T11:45:00+09:00
last_seen: 2026-07-16T15:16:32+09:00
first_seen: 2026-07-12T11:45:00+09:00
concern_id: redirect-off-a-follow-url-is
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Redirect off a follow URL is refused by the confined transport

## Description

FOLLOW-URL redirect refusal by the confined transport is unchanged; driver-http was not touched on this branch

## How to Fix

Implement redirect handling for FOLLOW URLs if security review approves

