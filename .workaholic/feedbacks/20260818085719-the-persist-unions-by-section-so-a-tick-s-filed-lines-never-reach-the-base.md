---
type: Feedback
title: The persist unions by section, so a tick's filed lines never reach the base
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T08:57:19+00:00
author: a@qmu.jp
supersedes: 
---

# The persist unions by section, so a tick's filed lines never reach the base

The housekeep tick log is the memory every later tick dedups against, and the `<step>-filed` half of it does not reach the base on its own.

`persist-log.sh` unions **by section**: for each `## <tick-id>` in the checkout it greps the base copy for `^## <tick>$` and, on a hit, `continue`s — the whole section is skipped. That is correct for two containers ticking under different tick ids, and wrong for the one case the skill guarantees, because the two halves of a tick are written at different times:

1. `run.sh` writes the nine step lines, then runs `persist-log.sh` as its closing act — the section lands on the base.
2. The agent then acts on `needs_agent` and appends `<step>-filed` lines **into that same section**.
3. Nothing persists again. A later `persist-log.sh` sees the section already present and reports `already_current` / `sections: 0` — truthfully by its own rule, and having carried none of the lines added in step 2.

Observed this tick (`20260818-085119`): the three filed lines were appended, `persist-log.sh --tick 20260818-085119` answered `{"persisted": true, "status": "ok", "reason": "already_current", "sections": 0, "changed": false}`, and the base carried none of them.

It is not new. `git log` on main shows a hand-authored "Carry the tick's filed lines to the base" commit after **each** of the 06:52 and 07:51 ticks — the same gap, patched by hand every hour, never recorded. A routine that needed a manual repair three ticks running is one whose audit trail depends on someone noticing.

What it costs, in the skill's own terms: `SKILL.md` calls the probe line and the filed line "a second, distinct fact" and says "both survive a re-entered tick", and `reference/workflow.md` §2 has the next tick's dedup read `inbound-sweep-filed` — which is exactly what does not arrive. A routine container is discarded, so an unpersisted filed line is gone, and the next tick re-files what this one already filed while the log reads as though it found nothing.

Two shapes would close it, and the choice between them is real:

- **Union at line level inside a matched section** rather than skipping the section — take the base's lines and append the checkout's that are absent. Concurrency stays safe (two containers write disjoint `` - `<step>`: `` lines), and one persist call then carries a tick however many times it is written to. This is what was done by hand here.
- **Persist a second time after the filing pass**, leaving the union rule alone — one extra line in the caller's contract, but it only helps if the union stops skipping matched sections, so it is not independent of the first.

Either way `already_current` should stop being reachable while the checkout holds a line the base does not: that report is what made three hours of loss read as three quiet successes.

(Recorded under `source: discussion` because `development`, which this record's channel actually is and which every neighbouring record carries, is rejected by the installed `create.sh` — filed separately.)
