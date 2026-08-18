---
type: Feedback
title: The stuck-prs read cap shrinks the count as the loop files its own clean pull requests
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T10:57:43+00:00
author: a@qmu.jp
supersedes: 
---

# The stuck-prs read cap shrinks the count as the loop files its own clean pull requests

The `stuck-prs` step and the `merge-conflicts` step share `pulls-state.sh`, whose default
`--limit 10` reads open pull requests newest-first and stops. The repository now has **14** open
pull requests, and the four newest (#75, #76, #77, #78) are this housekeeping tick own feedback
pull requests — all `clean`. They occupy four of the ten slots, evicting the four oldest
conflicted pull requests (#29, #46, #57, #64) from the read entirely.

## The measured effect

The hourly wrench posts in dev-qfs reported the count falling every hour, which reads as steady
progress:

- 15:55 JST — 8 pull request(s) waiting on a human (`stuck:4117867715`)
- 16:55 JST — 7 pull request(s) (`stuck:1592394930`)
- 17:53 JST — 6 pull request(s) (`stuck:2177705629`)
- 18:55 JST — 5 pull request(s) (`stuck:3784801314`)

Nothing was resolved. An untruncated read this tick (`pulls-state.sh --limit 20` ->
`total_open: 14, read: 14, truncated: false`) returns #29, #46, #57 and #64 as
`mergeable: false` / `mergeable_state: dirty` — still conflicted, exactly as at 15:55. The entire
decline is the read cap.

## Why it is self-reinforcing

The tick files a new feedback pull request most hours. Each one is clean, each one takes a slot,
and each one therefore evicts one more conflicted pull request from the window. **The loop own
output pushes the problems it exists to report out of its own field of view**, monotonically, and
the more it finds the blinder it gets.

## Why it also defeats the dedup

`stuck:<digest>` is computed over the truncated subset, so every eviction changes the digest. The
gate that exists to stop an unchanged answer being repeated fires a fresh post every hour for a
state that has not changed since 15:55 — while under-reporting it. This compounds the two findings
already recorded (`20260818065547` digest flap, `20260818075621` unknown-as-clean): the cap is a
third, independent cause, and the only one that is monotone and self-inflicted.

## Direction

Bound the per-pull reads by `total_open` rather than a fixed 10, or compute the digest and the
posted count over the full open set and read mergeability lazily. Failing that, a truncated read
must make the step report `degraded` — never a smaller count. The step contract already promises
"a busy repository is never silently half-read", and the cap *is* reported on the
`merge-conflicts` line, but it is not carried into the `stuck-prs` count, the digest, or the post
a human reads.

## One stale fact carried by all four posts

#72 was named as the failing-check entry every time. Its latest check run
(https://github.com/qmu/qfs/actions/runs/32035607096) is all-success; the failure belongs to the
superseded run 32035583794. Its only remaining blocker is uncomputed mergeability.
