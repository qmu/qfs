---
type: Feedback
title: The Prepare Release routine cannot write its draft release note: invalid GH_TOKEN in the routine container
kind: concern
source: slack
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T09:53:33+00:00
author: a@qmu.jp
supersedes: 
---

# The Prepare Release routine cannot write its draft release note: invalid GH_TOKEN in the routine container

The `[Prepare Release]` routine tick at 2026-08-18 18:48 JST posted its `📦 Release status` line to #dev-qfs with the draft-note field reading:

> Draft note: unavailable - draft release could not be created (invalid GH_TOKEN in the routine container)

Pointer: https://qmu.slack.com/archives/C0BM2ASB63G/p1787046490387809

The routine still computed and posted the release state, so the tick is not dead — but the half of it that keeps each target's draft release note current cannot write, and it has been failing quietly inside the post rather than as a reported precondition. `[Prepare Release]` is documented as keeping the draft note current on every tick, so an hourly routine is reporting a capability it does not currently have in its own container.

What this record does not decide: whether the fix is a re-provisioned `GH_TOKEN` for the routine environment, a switch to the same REST seam the rest of the loop uses (`gather/scripts/gh-rest.sh`), or a change to how the routine reports a credential failure (a named precondition rather than a line inside an otherwise-successful post). Noticed by the housekeeping tick reading the channel's own routine posts; nobody said this, so it is recorded as an observation, not as anyone's opinion.
