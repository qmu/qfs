---
created_at: 2026-07-03T06:30:00+09:00
author: a@qmu.jp
type: enhancement
layer: [UX]
effort: 0.5h
commit_hash: 109c5c4
category: Changed
depends_on: []
---

# Consent c/o choice should be a single keypress, unechoed

Owner feedback on the live v0.0.16 flow: pressing `c` should copy immediately — no Enter — and
the typed `c` should not remain on the prompt line. Mirror gmail-ftp's promptKey, minus the
deliberate key echo.

## Fix

`crates/qfs/src/tty.rs`: read the choice as ONE byte from `/dev/tty` in non-canonical,
echo-off mode (rustix termios safe API — the workspace forbids unsafe), restoring the terminal
state right after; keep ISIG so Ctrl-C works. Fall back to the echoed line read when raw mode
is unavailable (no controlling tty). Update the prompt text to drop the "+ Enter" wording.

## Quality Gate

- PTY e2e drives the real binary: a bare `c` byte (no newline) triggers the OSC 52 escape and
  the "Copied to your local clipboard." confirmation, and the wrong-state paste is still
  rejected before any exchange.
- Workspace tests / clippy / fmt / gen-docs / gen-skills green; owner confirms the feel on the
  local build before this ships.
