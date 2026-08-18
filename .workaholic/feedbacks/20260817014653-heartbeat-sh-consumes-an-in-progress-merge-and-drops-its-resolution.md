---
type: Feedback
title: heartbeat.sh consumes an in-progress merge and drops its resolution
kind: instruction
source: discussion
subject: observer_ai:[Implement] routine (unit batch-20260729145625)
created_at: 2026-08-17T01:46:53+00:00
author: a@qmu.jp
supersedes: 
---

# heartbeat.sh consumes an in-progress merge and drops its resolution


Measured in this repository on 2026-08-17 by the `[Implement]` routine, driving unit
`batch-20260729145625` (PR #46).

**What happened.** The unit's branch had gone un-mergeable against `main`, so the run
opened the catch-up merge in the claim worktree, resolved five conflicted files
(four version-bump files plus `Cargo.lock`) and staged them, leaving `MERGE_HEAD` set
and the resolved tree in the index. Before committing, the run fired the routine
heartbeat from the main checkout:

    bash <src>/skills/drive/scripts/heartbeat.sh batch-20260729145625
    {"beat": true, "unit": "batch-20260729145625", "branch": "claude/existing-prs-issues-naiqyz", "reason": ""}

That beat became commit `0a66fbd`, subject `Refresh heartbeat` — and it is a **merge
commit**: `git rev-list --parents -n1 0a66fbd` returns two parents, the branch tip
`d9b28dd` and `main` at `cd8be38`. Its tree is the pre-merge tree, because
`commit.sh --allow-empty` builds against a scratch index seeded from `HEAD`. So the
beat consumed `MERGE_HEAD`, recorded `main` as merged, and carried none of `main`'s
content — an evil merge that reverts every change `main` made since the fork. It was
pushed immediately by the heartbeat's own non-blocking push, so the history could not
be rewritten (force-push is on the run's safety floor). The staged resolution survived
in the index and was recoverable as a follow-up commit, but only because the run
noticed.

**Why the existing guard does not cover this.** `drive/reference/claims.md`
(*Heartbeat mechanics*) already states the scratch-index design and its reason — a
beat fired over a staged `git rm` once swept real deletions into a `Refresh heartbeat`
commit. That guard protects the *index*. It does not protect `MERGE_HEAD`: git's
`commit` still reads the pending merge parents from the repository, so the flag that
makes the commit changeless is exactly what makes it a **content-dropping** merge
rather than a harmless one. A heartbeat over a `CHERRY_PICK_HEAD`, `REVERT_HEAD` or
rebase-in-progress has the same shape.

**Where the repair surface is.** `skills/commit/scripts/commit.sh` (the
`--allow-empty` path) and/or `skills/drive/scripts/heartbeat.sh`, both in
`qmu/workaholic`. The narrow fix is for the empty-commit path to refuse — or to skip
the beat, reporting a reason such as `merge_in_progress` — when
`MERGE_HEAD`/`CHERRY_PICK_HEAD`/`REVERT_HEAD` or `rebase-merge`/`rebase-apply` exists
in the worktree's git dir. A missed beat is already documented as reported-never-fatal;
a beat that eats a merge is not recoverable once pushed.

**Also worth carrying to that repair.** `drive/SKILL.md` §4 tells the run to beat
"roughly every ten minutes or once per ticket", and a mid-drive index is exactly when a
catch-up merge is open — so this is on the ordinary path, not an exotic one.
