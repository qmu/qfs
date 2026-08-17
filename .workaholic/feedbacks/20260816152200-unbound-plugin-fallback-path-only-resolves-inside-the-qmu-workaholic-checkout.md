---
type: Feedback
title: Unbound-plugin fallback path only resolves inside the qmu/workaholic checkout
kind: instruction
source: discussion
subject: observer_ai:[Implement] routine (issue #44, claude[bot])
created_at: 2026-08-16T15:22:00+00:00
author: a@qmu.jp
supersedes: 
---

# Unbound-plugin fallback path only resolves inside the qmu/workaholic checkout

Source: https://github.com/qmu/qfs/issues/44 (filed by claude[bot], 2026-08-14)

**What was reported.** A scheduled routine bound to the `qmu/qfs` checkout tried to run
`Skill(implement)`. The workaholic plugin was enabled but the skill did not load ("Unknown
skill", and skill search found nothing) — the known unbound-in-session precondition. The
documented escape hatch for that case (qmu/workaholic#448) tells the session to read the
skills as plain files under `plugins/workaholic/skills/` and run their scripts directly,
e.g. `bash plugins/workaholic/skills/check-deps/scripts/plugin-src.sh`. That path only
exists inside the `qmu/workaholic` checkout itself. From `qmu/qfs` the `plugins/` directory
holds only the qfs plugin, so the script exits 127; the session's GitHub scope was
restricted to `qmu/qfs`, so workaholic source could not be fetched another way, and no
workaholic source existed anywhere on the container. Nothing was claimed, cut or pushed;
all 14 tickets stayed in `.workaholic/tickets/todo/`.

The reporter's conclusion: for every non-workaholic repository the unbound-plugin case is
a hard stop rather than a warn-and-continue, and the fallback should reach a source that
exists off-checkout — a vendored copy, an npm/plugin cache, or a widened routine repo
scope — instead of a sibling path that only resolves inside `qmu/workaholic`.

**Reproduced in this repository on 2026-08-16, with the outcome now different.** This
`[Propose]` tick hit the same precondition: `Skill(workaholic:propose)` returned "Unknown
skill" even though the SessionStart hook had just reported "workaholic installed. Run
/reload-plugins if its commands aren't available yet." The routine prompt's fallback
(`bash plugins/workaholic/skills/check-deps/scripts/plugin-src.sh`) exited 127 exactly as
reported. The run recovered by locating the plugin off-checkout by hand:

    /root/.claude/plugins/cache/workaholic/workaholic/1.0.178/skills/check-deps/scripts/plugin-src.sh

which then reported `{"ok": true, "src": ".../1.0.178", "source": "registry"}`. So the
resolver the fallback wants already exists and already resolves correctly off-checkout; the
only broken part is the *path the prompt uses to reach it*. Two off-checkout copies were
present on this container (`~/.claude/plugins/cache/workaholic/workaholic/<version>/` and
`~/.claude/plugins/marketplaces/workaholic/plugins/workaholic/`), so a glob over
`~/.claude/plugins/` would have resolved on the first try.

**Where the repair surface actually is.** The broken sentence is not a `qmu/qfs` file. It is
the shared routine prompt template shipped by the plugin —
`skills/workaholify/routines/{implement,fb,release-status}.md`, one identical line in each —
plus whatever the qmu/workaholic#448 escape-hatch documentation says. Both live in
`qmu/workaholic`. This repository owns only the installed copy of
`.claude/hooks/session-start.sh`, whose own header records that editing it here is drift
(`/workaholify` reports against the canonical copy), and that hook is not where the defect
is: the hook succeeded on this tick and the fallback still failed.

**Also worth carrying to that repair.** PR #48 (2026-08-16) added this repository's
SessionStart bootstrap, so the plugin now installs on every cloud tick — and this tick shows
that installing it is not sufficient: the session's skill registry is built before the hook
runs, so a routine still lands in the unbound state and still needs the fallback. The
fallback is therefore the load-bearing path for cloud routines in consuming repositories,
not a rare last resort.
