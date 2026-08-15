---
created_at: 2026-08-15T13:05:10+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
merge_policy: review
claim: work-20260815-130504
---

# Bootstrap web sessions so the scheduled routines can run

## Overview

The `[Propose] qfs` and `[Implement] qfs` account routines (created 2026-08-14, hourly at
`:15` / `:30`) fire on time and stop immediately at their own precondition. The 2026-08-14
19:44 JST tick posted `⚪ Paused - unbound_in_claude_session` to `dev-qfs`: `Skill(implement)`
fails with Unknown skill because a Claude Code Web session starts in a fresh container where
nothing installs the workaholic plugin, and the routine prompt's fallback
(`bash plugins/workaholic/skills/check-deps/scripts/plugin-src.sh`) exits 127 because this
checkout's `plugins/` holds only the qfs plugin (filed as issue #44). The workaholify gateway's
web-bootstrap check (`check-bootstrap.sh`) reports all four problems against this repository:
`hook_missing`, `not_registered`, `enabled_plugin`, `marketplace`.

## Policies

- workaholic:development / policy-as-plugin — the rules load via the plugin, so the repository
  carries only the thin bootstrap that installs the plugin in a fresh web container; nothing of
  the standards is copied into this repo.
- workaholic:development / overnight-ai-runs — an unattended routine that fires and silently does
  nothing reads as healthy; the bootstrap is what turns a configured routine into a working one.
- workaholic:operation / delivery — the fix ships like any change: topic branch, PR, patch bump,
  tag on ship; the deliverable is the merged bootstrap on main, which every cloud checkout clones.

## Implementation direction

Install the workaholify §4 web bootstrap, byte-for-byte from the plugin's canonical copy, the
same shape qmu/workaholic itself carries:

- `.claude/hooks/session-start.sh` — the canonical `bootstrap/session-start.sh` from the
  workaholic plugin (v1.0.177), unmodified so `matches_canonical` holds.
- `.claude/settings.json` — a `SessionStart` entry running the hook (matcher `startup`,
  timeout 120), `enabledPlugins: workaholic@workaholic`, and the `workaholic` entry in
  `extraKnownMarketplaces` (github qmu/workaholic).
- `.claude/git-identities` — `tamurayoshiya=a@qmu.jp`, so the bootstrap hook can set the
  repo-local git identity and the developer's own [Implement] routine can claim tickets
  assigned to them (ownership keys on `git config user.email`).

Bump the patch version (0.0.97 → 0.0.98) per the shipped-PR rule.

## Quality Gate

- Acceptance: `bash <plugin>/skills/workaholify/scripts/check-bootstrap.sh` on this repository
  reports `ok: true` with an empty `problems` list — all four 2026-08-14 findings
  (`hook_missing`, `not_registered`, `enabled_plugin`, `marketplace`) cleared, and the installed
  hook `matches_canonical` byte-for-byte.
- Verification: run check-bootstrap.sh before commit; after merge, the next routine tick must
  reach its command instead of posting `unbound_in_claude_session` (checked on the routine's own
  Slack post or its run log via list_runs).
- Gate: no Rust surface changes, so the standard gates are unaffected; the patch bump
  (0.0.97 → 0.0.98) rides the ship tag as usual.
