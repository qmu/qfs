---
type: Concern
origin_pr: 21
origin_pr_url: https://github.com/qmu/qfs/pull/21
origin_branch: work-20260705-032203
origin_commit: 1140091
created_at: 2026-07-05T13:59:54+09:00
last_seen: 2026-07-05T13:59:54+09:00
first_seen: 2026-07-05T13:59:54+09:00
concern_id: declared-driver-live-read-apply-eagerly
severity: low
status: resolved
resolved_by_pr: 
resolved_by_commit: 3d43e99
---

# Declared-driver live read/apply eagerly opens the credential store

## Description

The evaluator's live read/apply wiring (`commit.rs::live_registry`, ticket 145137, commit 2ca3a04) eagerly opens the credential store whenever a declared driver is connected, rather than lazily binding it only when a request actually needs a secret — the same quiet-eager-bind shape already flagged for the commit-side cloud apply registry (carried concern above, PR #15).

## How to Fix

Apply the cloud facets' lazy-bind, prompt-at-proven-need pattern to the declared-driver connect path once it's implemented for `commit.rs`'s cloud apply drivers generally, so the two converge on one fix.
