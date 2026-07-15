---
type: Concern
mission:
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
created_at: 2026-07-12T11:45:00+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-12T11:45:00+09:00
concern_id: redirect-off-a-follow-url-is
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Redirect off a follow URL is refused by the confined transport

## Description

A redirect off a FOLLOW download URL (cross-host download) is refused by the confined transport, fail-closed (see 4de2f42 in `packages/qfs/crates/driver-http`). May need revisiting if a real service redirects downloads. (`redirect-off-a-follow-url-is.md`, origin `c30fa0a`)

## How to Fix

Monitor live service redirect behaviors during the live rounds; revisit the transport policy if real services require redirect following.

