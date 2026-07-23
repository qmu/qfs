---
type: Concern
concern_id: owner-attended-live-verification-backlog
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: []
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
created_at: 2026-07-16T21:18:59+09:00
first_seen: 2026-07-05T01:25:53+09:00
last_seen: 2026-07-23T23:59:51+09:00
severity: moderate
status: active
compound: true
resolved_by_pr: 
resolved_by_commit: 
---

# Owner-attended live verification backlog

## Description

The standing queue of live, owner-attended confirmations that hermetic tests cannot replace, gathered from eight concerns (2026-07-16 triage, owner-directed): the three-step vault-unlock check on the headless host; the six remaining live rounds (Slack post, Gmail reply, /ghdecl read, and siblings); the live /chatwork read confirming the newer view body after replace-on-install; the post-upgrade sanity read confirming the one-shot config-registry copy carried the live registry into the System DB; the bearer-gated non-loopback plan/apply round; the Cloudflare Artifacts beta create/clone/delete round-trip with the sealed repo token; the Cloudflare/Postgres/Drive live provider acceptance that needs owner credentials unavailable in-container; and the standing fact that live-only provider gates sit outside local proof by design. None of these is code work; each is an attended session on the operator's box.

## How to Fix

Run the rounds in owner-attended sessions, checking items off this backlog as evidence lands on the relevant archived tickets; split a member back out only if one grows its own code work.

