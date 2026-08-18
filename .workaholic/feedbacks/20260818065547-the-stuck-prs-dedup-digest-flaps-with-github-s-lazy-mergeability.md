---
type: Feedback
title: The stuck-prs dedup digest flaps with GitHub's lazy mergeability
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T06:55:47+00:00
author: a@qmu.jp
supersedes: 
---

# The stuck-prs dedup digest flaps with GitHub's lazy mergeability

Step 6 (`stuck-prs`) keys its reminder on `stuck:<digest>`, a hash over the sorted
`<number>:<blocked_by>` set, and the notify model's second gate is "no earlier post for
this exact state". `blocked_by` is derived from GitHub's `mergeable` field, which the
contract correctly maps `null → unknown`, never `clean`.

But `mergeable` is **lazily computed**. Two reads of the same repository, roughly one
minute apart inside tick `20260818-065210`, produced two different answers:

| read | result | digest |
| --- | --- | --- |
| 1st (the tick's own) | 7 × `conflict` (#29, #46, #57, #59, #64, #65, #71) + #72 `checks` | `stuck:4117867715` |
| 2nd (a re-probe, same tick) | 10 × `unknown` (#29, #46, #50, #57, #59, #64, #65, #71, #72, #74) | `stuck:400185366` |

Nothing about the pull requests changed between the two reads. GitHub simply had a
background mergeability recompute in flight, and the second read caught every pull
request mid-recompute.

The consequence is that the dedup key is a function of GitHub's cache warmth rather
than of the repository's state. `unknown ⇄ conflict` flapping produces a new digest,
a new "distinct state", and therefore a fresh top-level `🔧 Needs a decision` root —
potentially every hour, for an answer that has not changed. That is precisely the
noise the content-keyed dedup was introduced to prevent, and unlike the red-alert
cool-down there is no time window behind it to absorb the flap.

Candidate directions, none taken here:

- **Exclude `unknown` from the digest.** A pull request whose mergeability GitHub has
  not computed is not a known state, so it should neither create nor perturb one. The
  step already tells the agent to "re-read before acting"; the key should say the same.
- **Re-read the `unknown` rows once** before digesting, which is what a human acting on
  the row is told to do anyway, at the cost of a few more single-pull reads inside the
  existing `--limit` budget.
- **Digest only the actionable subset** (`conflict`, `review`, `checks`, `draft`,
  `behind`), leaving `unknown` out of both the key and the count.

The first is the cheapest and matches the contract's own stance that unknown is
unknown. Whichever is chosen, the count in the post should agree with the digest: this
tick posted "8 pull request(s)" from the first read while the second read saw 10, and
a human reading two consecutive posts would have no way to tell that nothing moved.

Observed by the `[Housekeep]` routine, tick `20260818-065210`, session
https://claude.ai/code/session_011tPwsczYpbTsrsAqn8y8bP
