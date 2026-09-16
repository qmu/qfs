---
type: Feedback
title: A pull request whose checks are still running is reported as one with a failing check
kind: concern
source: development
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-19T04:55:32+00:00
author: a@qmu.jp
supersedes: 
---

# A pull request whose checks are still running is reported as one with a failing check

`pulls-state.sh` maps `mergeable_state` to `blocked_by` with a single unconditional
arm — `unstable) blocked=checks` (line 99) — and step 6's contract renders `checks` as
"the author must fix a failing check or say it is expected".

But GitHub's `unstable` means *mergeable, with non-required checks not currently green*,
and "not green" includes **still running**. A pull request whose CI is merely in flight
is therefore reported as one whose CI failed, and it enters the actionable set, the
count, and the `stuck:<digest>` key on that basis.

## Measured, tick `20260819-045123`

The step reported three stuck pull requests — `92:conflict 98:checks 99:checks`, keyed
`stuck:2409608402`. Reading the check runs on each head resolves it:

| pull | head | check runs on the head |
| --- | --- | --- |
| #98 | `a134402` | 15 completed **success**, 1 `in_progress` (`cross-compile (aarch64-unknown-linux-gnu)`), **0 failures** |
| #99 | `bab326e` | 15 completed **success**, 1 `in_progress` (same job), **0 failures** |

Neither author has anything to fix. The honest actionable set this tick is `{92:conflict}`
— which digests to `stuck:3207736273`, a key the channel already carries from 06:54 JST,
so the correct outcome was silence and the step's own key would have bought a fourth post
about one unchanged conflict.

This is not the first sighting: tick `20260818-195200` recorded the same shape for #88
("two check runs still `in_progress` with zero failures") and suppressed on the same
reasoning. Twice measured, both times caught only because an agent re-read the check runs
by hand.

## Distinct from the three defects already filed

- `20260818065547` — the digest flaps with GitHub's **lazy mergeability** (`null ⇄ conflict`).
- `20260818075621` — `merge-conflicts` renders **unknown** as none-conflicted.
- `20260818105743` — the ten-pull **read cap** shrinks the count.

All three are about `mergeable`; this one is about `mergeable_state`, and it survives every
fix to those: a fully computed, non-null, *correct* `unstable` still reads as a failing check.

## Candidate directions, none taken here

- **Resolve `unstable` against the head's check runs** before calling it `checks`:
  `GET /repos/{o}/{r}/commits/{sha}/check-runs`, and count only runs whose `conclusion` is
  `failure`/`timed_out`/`cancelled`/`action_required`. Zero such runs with at least one
  `in_progress`/`queued` is a *pending* state, not an actionable one — the same class as
  `unknown`, which the contract already says to re-read rather than act on. Costs one read
  per `unstable` pull, inside the existing `--limit` budget.
- **Add a `pending` value to `blocked_by`**, excluded from the digest and the count exactly
  as the candidate directions on `20260818065547` propose for `unknown`, so a tick can still
  *report* the state without paging anyone for it.
- **Leave the map and fix the render**: state the uncertainty in the post's wording. Cheapest,
  and the weakest — the count and the digest stay wrong, so the one-post-per-state gate stays
  defeated.

The first is the one that makes the count and the key true at the same time, and it also
supersedes the superseded-run confusion tick `20260818-105153` had to reason around by hand.

Observed by the `[Housekeep]` routine, tick `20260819-045123`, session
https://claude.ai/code/session_01Pm2C38UowoPrcVmEPiWQJW
