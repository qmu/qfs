---
type: Feedback
title: Workaholic unbound-plugin fallback assumes the checkout is qmu/workaholic
kind: instruction
source: discussion
subject: observer_ai:[Implement] routine (claude[bot], qmu/qfs#44)
created_at: 2026-08-16T05:19:48+00:00
author: noreply@anthropic.com
supersedes: 
---

# Workaholic unbound-plugin fallback assumes the checkout is qmu/workaholic

Source: https://github.com/qmu/qfs/issues/44

**Context**

A scheduled routine tried to run `Skill(implement)` in a session bound to the qmu/qfs
checkout. The workaholic plugin was enabled (ListPlugins showed its id) but the skill
failed "Unknown skill" and skill search found nothing — the known "unbound-in-session"
precondition, whose documented repair is a fresh session, not a retry. No ticket was
claimed, no branch cut, nothing pushed; all 14 tickets remained in
`.workaholic/tickets/todo/`.

**The problem reported**

The documented escape hatch for that case (from FB "Routines should keep going when the
plugin is unbound", qmu/workaholic#448) says to read skills as plain files under
`plugins/workaholic/skills/` and run their scripts directly, e.g.
`bash plugins/workaholic/skills/check-deps/scripts/plugin-src.sh`. That assumes the
checkout IS the qmu/workaholic repo. It does not resolve from qmu/qfs (or any other
consuming repo): this repository vendors only the qfs plugin under `plugins/`, so the
path exits 127. The reporter also found the session GitHub scope restricted to qmu/qfs
and no workaholic source anywhere on the container.

**Conclusion the reporter drew**

For every non-workaholic repo the unbound-plugin case is a hard stop rather than the
warn-and-continue it is documented to be. The fallback should reach a source that exists
off-checkout — the installed plugin cache, a vendored copy, or a widened repo scope —
rather than a sibling path that only resolves inside qmu/workaholic itself.

**Triage already on the issue (2026-08-14, qmu/qfs#44 comment)**

Confirmed as a real gap, but not actionable in this repository: nothing in the qfs
codebase can make `plugins/workaholic/skills/**` resolve. The fix belongs in the
workaholic plugin's own recovery guidance, so the issue should be transferred to
qmu/workaholic (or closed here and re-filed there) and linked to qmu/workaholic#448.
