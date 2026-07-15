---
created_at: 2026-07-03T15:04:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure, UX]
effort: 1h
commit_hash: 860b10c
category: Changed
depends_on: []
---

# Installed qfs plugin (cache 0.1.0) teaches the retired connection namespace

The Claude-plugin cache on this host (`~/.claude/plugins/cache/qfs/qfs/0.1.0/skills/qfs-gmail`)
still carries the pre-ADR-0008 setup: `qfs connection add google-app/gmail`,
`QFS_GOOGLE_CONSENT=1`, `qfs identity signup`, and the ssh -L loopback port-forward — ALL retired
or replaced (per-layer verbs, paste-back consent). An agent following the installed skill's Setup
on v0.0.17 fails at the first command. The repo's regenerated skills are current; the published
plugin was never re-versioned after the hard breaks.

## Fix

Bump the plugin version in `plugins/qfs/.claude-plugin/plugin.json` (or marketplace manifest) and
republish so installs pick up the regenerated skills; establish the rule that a shipped hard
break of any CLI surface the skills mention re-versions the plugin in the same PR (the gen-skills
ratchet keeps content true, but only a version bump propagates it to installs). Verify the
installed cache updates on this host.

## Key files

- `plugins/qfs/.claude-plugin/` (manifest/version), the marketplace repo entry, CLAUDE.md
  (versioning rule note if adopted)

## Quality Gate

- A fresh plugin install/update on this host serves skills whose Setup matches the v0.0.17 verbs
  (paste-back consent, app/account/connect).
