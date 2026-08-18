---
type: Feedback
title: The tick log's -filed lines never reach the base, so dedup is dead
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T06:57:37+00:00
author: a@qmu.jp
supersedes: 
---

# The tick log's -filed lines never reach the base, so dedup is dead

The tick log's `<step>-filed` lines are the loop's dedup memory. `reference/workflow.md`
says so for step 2 ("What it filed is recorded as `inbound-sweep-filed`, which the next
tick's dedup reads") and for step 7 ("A finding an earlier tick logged under
`doc-drift-filed` is counted and dropped"). In a routine-fired container, none of those
lines ever reaches the base.

The sequence is forced by the contract, not by a mistake in the run:

1. `run.sh` runs the nine steps, then runs `persist-log.sh` as its **closing act**. At
   that moment the section `## <tick-id>` contains only the nine probe lines.
2. `persist-log.sh` unions **by section**: "whatever `## <tick-id>` sections the base
   already carries are left untouched, and only the sections this checkout has and the
   base does not" are appended. The section is now on the base.
3. The agent then acts on `needs_agent` and appends `<step>-filed` lines — *into the
   same section*, which is exactly what the log's `(tick, step)` idempotence is designed
   for ("a second, distinct fact from the probe's own line").
4. A second `persist-log.sh` reports `already_current`, `sections: 0`, `changed: false`.
   The section on the base is the step-3-less version, permanently.
5. The container is discarded.

Measured on tick `20260818-065210`: three `-filed` lines were appended
(`inbound-sweep-filed`, `stuck-prs-filed`, `human-checkin-filed`), a second persist
returned `{"persisted": true, "status": "ok", "reason": "already_current", "sections": 0,
"changed": false}`, and `git show origin/main:.workaholic/housekeeping/2026-08-18.md`
carries the nine probe lines and none of the three.

So every hour, a routine tick re-derives findings an earlier tick already filed, and
`log-read.sh --step <step>-filed` — the documented instrument for "did an earlier tick
already file this?" — answers `count: 0` forever. Step 7's dedup is called
"not optional"; on the base it does not exist. A hand-run never sees this, because the
developer's checkout keeps the lines: it is the same asymmetry `persist-log.sh`'s own
header identifies as the reason the persist exists at all.

Candidate directions, none taken here:

- **Union by line, not by section.** Append the `(tick, step)` lines the base is missing
  within an existing section, rather than skipping any section already present. This
  keeps the concurrency property — two containers appending different `(tick, step)`
  keys still union — and is the smallest change.
- **Persist twice, by contract.** Keep the closing act, and have the SKILL instruct the
  agent to re-run `persist-log.sh` after the last `-filed` line. This needs the
  by-line union anyway, so it is not an alternative to the first, only a trigger for it.
- **Move the persist after the filing.** Rejected as stated: `persist-log.sh`'s header
  explains that persisting early is what lets a tick that dies half-way still record
  what it observed.

The first plus the second is the coherent pair: union by line so a re-persist can add to
a section, and re-persist once the filing is done.

Observed by the `[Housekeep]` routine, tick `20260818-065210`, session
https://claude.ai/code/session_011tPwsczYpbTsrsAqn8y8bP
