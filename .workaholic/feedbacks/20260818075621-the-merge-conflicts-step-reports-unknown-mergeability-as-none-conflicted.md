---
type: Feedback
title: The merge-conflicts step reports unknown mergeability as none conflicted
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T07:56:21+00:00
author: a@qmu.jp
supersedes: 
---

# The merge-conflicts step reports unknown mergeability as none conflicted

Step 4 (`merge-conflicts`) reported this tick:

> `11 open pull request(s), none conflicted (read cap 10, truncated: true)`

Six of those eleven were conflicted at that same moment. Read directly over REST minutes
later, `#46`, `#57`, `#59`, `#64`, `#65` and `#71` all carry `mergeable: false`,
`mergeable_state: dirty`; step 6 of the *same tick* reported them as `conflict` in its own
line. So the tick log now carries two lines that contradict each other, and the one a human
skims first says the repository is clean.

The step's own contract forbids exactly this: *"GitHub's lazily-computed `mergeable: null`
is `unknown`, never `clean`"* (`reference/workflow.md` §4). `pulls-state.sh` honours it —
it emits `blocked_by: "unknown"` — but `step-merge-conflicts.sh` then greps only for
`"blocked_by": "conflict"` and, finding none, prints **"none conflicted"**. The rule is
kept in the data and thrown away in the sentence. A tick where every row was unknown and a
tick where every row was genuinely mergeable produce the identical summary.

## Why it is systematic, not a fluke

`reference/workflow.md` §6 says `pulls-state.sh` is *"resolved once per tick, used twice"*.
It is not: `step-merge-conflicts.sh` and `step-stuck-prs.sh` each invoke it separately.
Step 4 runs first, and its read is the one that triggers GitHub's background mergeability
computation — so step 4 gets `null` for rows that have gone cold, and step 6, a minute
later against the warmed cache, gets the truth. The ordering makes step 4 the step that
reliably reads nulls, which means the false "none conflicted" is the expected outcome of a
quiet repository, not a rare race.

## Directions, none taken here

- **Say unknown out loud.** When no row is `conflict` but some are `unknown`, the summary
  should read `N open pull request(s), 0 conflicted, M unknown (GitHub has not computed
  mergeability)` and the status should not be a bare `ok`. Cheapest fix, and it is what the
  contract already says.
- **Share the read, as §6 already claims it does.** Resolve `pulls-state.sh` once per tick
  and hand the same JSON to both steps. This removes the ordering asymmetry entirely and
  halves the API reads; it also makes steps 4 and 6 incapable of disagreeing.
- **Re-read the unknown rows once** before either step reports, inside the existing
  `--limit` budget — the same remedy the sibling record proposes for the digest.

The second direction subsumes the sibling concern
`20260818065547-the-stuck-prs-dedup-digest-flaps-with-github-s-lazy-mergeability`: one
shared read per tick gives one state, so neither the digest nor the summary can flap
against the other.

Observed by the `[Housekeep]` routine, tick `20260818-075137`, session
https://claude.ai/code/session_01Vi5qLkyAbJBZWrZsU6e4gd
