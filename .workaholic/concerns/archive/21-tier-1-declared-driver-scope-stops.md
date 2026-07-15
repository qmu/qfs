---
type: Concern
origin_pr: 21
origin_pr_url: https://github.com/qmu/qfs/pull/21
origin_branch: work-20260705-032203
origin_commit: 1140091
created_at: 2026-07-05T13:59:54+09:00
last_seen: 2026-07-05T13:59:54+09:00
first_seen: 2026-07-05T13:59:54+09:00
concern_id: tier-1-declared-driver-scope-stops
severity: moderate
status: resolved
resolved_by_pr: 
resolved_by_commit: fbc97b8
---

# Tier-1 declared-driver scope stops short of view-body-expansion, per-map IRREVERSIBLE, and redirect confinement

## Description

The evaluator (ticket 145137, commit 2ca3a04) deliberately ships tier-1 only: a declared read/write is a native RestDriver read/write with no view-body-expansion engine, so a post-decode pipe op beyond tier-1, honoring the per-map `IRREVERSIBLE` gate, and a reqwest redirect-policy layer scoped to the declared driver's confined host are all named parks rather than implemented behavior. Redirect confinement matters because reqwest follows 30x redirects internally (`client.rs:66`) and the `send_one` chokepoint guard does not see a redirect target before reqwest follows it.

## How to Fix

Scope a `redirect::Policy` (none, or host-checking) to the declared driver's transport so a 30x cannot leave the confined host, wire the per-map `IRREVERSIBLE` flag through the MAP apply path, and add the post-decode pipe-op layer once a concrete declared driver needs it beyond tier-1.
