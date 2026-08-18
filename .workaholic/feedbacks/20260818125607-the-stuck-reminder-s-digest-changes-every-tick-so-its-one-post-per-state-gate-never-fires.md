---
type: Feedback
title: The stuck reminder's digest changes every tick, so its one-post-per-state gate never fires
kind: concern
source: slack
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T12:56:07+00:00
author: a@qmu.jp
supersedes: 
---

# The stuck reminder's digest changes every tick, so its one-post-per-state gate never fires

Measured on 2026-08-18 from the #dev-qfs channel: the housekeeping tick posted **seven**
`🔧 Needs a decision` lines between 15:55 and 20:56 JST, each under a different `stuck:<digest>`
key, all describing one standing backlog of conflicted pull requests.

Pointers (key + first line only — no message bodies quoted):

- 15:55 JST `stuck:4117867715` — 8 pull request(s) waiting on a human
- 16:55 JST `stuck:1592394930` — 7 pull request(s) waiting on a human
- 17:53 JST `stuck:2177705629` — 6 pull request(s) waiting on a human
- 18:55 JST `stuck:3784801314` — 5 pull request(s) waiting on a human
- 19:59 JST `stuck:2907503066` — 7 pull request(s) waiting on a human
- 20:52 JST `stuck:4040151564` — 3 pull request(s) waiting on a human
- 20:56 JST `stuck:403587264` — 11 pull request(s) waiting on a human

## The mechanism

`stuck-prs` digests the sorted `<number>:<blocked_by>` set. `blocked_by` derives from GitHub's
**lazily computed** `mergeable` field, which flips between `conflict` and `unknown` for the same
branch from one read to the next — the untruncated read at 21:00 JST shows 11 of 15 open pull
requests as `mergeable: null`, while #65/#71 read `conflict` an hour earlier. The 10-pull read
cap (already filed as `20260818105743`) varies the membership of the set on top of that.

So the digest is a fresh value on nearly every tick, and the gate `workaholic:notify` states as
"no earlier post for this exact state" never fires. The design intent — one reminder per distinct
state, an unchanged answer never repeated — is defeated by the key, not by the reader. Seven posts
in one evening for one unchanged backlog is the "channel full of plausible noise" failure the
notification model names by that phrase.

## What this is not

Distinct from the two defects already filed today. `20260818075621` is `merge-conflicts` rendering
`unknown` as "none conflicted" — a truthfulness bug in a different step. `20260818105743` is the
`stuck-prs` read cap shrinking the count. Both make an individual post *wrong*; this one makes the
*dedup* inoperative, and fixing either of those leaves it inoperative.

## Recommendation (recorded, not performed)

Key the digest on the pull-request **number set alone**, dropping `blocked_by`, and exclude
`unknown` members from the set entirely — an uncomputed mergeability is not a state a human can
act on. Both are small changes in `step-stuck-prs.sh`, and together they make the key stable
across a mid-recompute window while still posting when the actual backlog changes.

## What this tick did about it

Tick `20260818-125131` suppressed its own would-be eighth post on these grounds: its
`stuck:1889233892` set (#65 #71 #72 #74) is a strict subset of the 20:56 JST post four minutes
earlier, so the exact-string gate passed while the substance was already on the channel. The
suppression is recorded in the tick log under `stuck-prs-filed`; nothing was merged, rebased or
pushed to any pull request's branch.

## One incidental drift, recorded here rather than as its own record

`feedback/scripts/create.sh`'s usage header documents `source: meeting | slack | discussion |
development`, but its `case "$SOURCE"` accepts only `meeting|slack|discussion` and refuses
`development` with `bad_source`. This record was written as `slack` — honest, since the
measurement came from reading the channel — but a caller following the header fails.
