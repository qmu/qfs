---
type: Feedback
title: A re-landed claim branch loses its artifact attribution
kind: instruction
source: discussion
subject: observer_ai:[Implement] routine (unit batch-20260729145625)
created_at: 2026-08-17T01:48:09+00:00
author: a@qmu.jp
supersedes: 
---

# A re-landed claim branch loses its artifact attribution

Measured in this repository on 2026-08-17 by the `[Implement]` routine.

Unit `batch-20260729145625` was originally claimed on `work-20260729-145625`, whose PR
(#29) went stale. A later session re-landed the same commits on a fresh branch,
`claude/existing-prs-issues-naiqyz`, and opened PR #46 for it. The `Claim
batch-20260729145625` commit rode along as an ancestor, so the claim scan still reads the
unit from the new branch — but the unit's one ticket, archived at
`.workaholic/tickets/archive/work-20260729-145625/20260728085253-…md`, still carries
`claim: work-20260729-145625` in its frontmatter, naming the branch it was stamped on
rather than the branch that now holds the claim.

**Two visible consequences, both measured on this tick.**

1. `plan-units.sh` reported the claim with `artifacts: []`, so nothing linked the claim to
   its ticket. The base-side copy at `.workaholic/tickets/todo/20260728085253-…md` was
   therefore offered as **unclaimed backlog**, with no `excluded[]` row — a survey inviting
   a fresh runner to re-implement work that is finished and sitting in an open PR. This is
   exactly the double failure `drive/reference/claims.md` already names ("An empty artifact
   list is two failures at once", 2026-08-04); the re-land is a second way to reach it.
2. With no artifacts, `claims_has_work`'s conservative "no artifacts means unknown" branch
   called the drained unit `resumable`, so every tick resumed it and added an empty `Resume
   a PR-unit` commit. Nine such commits are on the branch. The correct verdict for this unit
   is `queue_drained` — its queue is drained and its PR waits on a human ruling (a `size`
   finding at `override` tier).

**Repair surface**: `qmu/workaholic` — either the scan (`drive/scripts/lib/claims.sh`)
attributing an artifact whose `claim:` names any branch in the claim's own ancestry, or a
re-land path that re-stamps the artifacts onto the new branch. This tick worked around it
by hand-correcting the stamp to `claim: claude/existing-prs-issues-naiqyz`, which is
accurate but is not a repair.
