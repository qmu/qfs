---
created_at: 2026-07-22T21:30:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort:
commit_hash:
category: Changed
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# claude launcher: RETURNING id captures the wrong stdout line from `claude --bg`

## Overview

Minted from the in-container launch **live fire** (ticket 20260717010600 QG2 / live-round
20260719231005). The launch path now works end to end against a real `claude` 2.1.217: PREVIEW is
effect-free and marks the INSERT irreversible, `--commit` alone fails closed
(`irreversible_ack_required`), and `--commit --commit-irreversible` spawns a real `claude --bg`
session that then appears in `/hosts/local/claude/sessions`. Verified live in the sanctioned
container (transcript in the live-round ticket).

But the launcher's **session-id capture is wrong for the real CLI**. `ClaudeCliLauncher::launch`
(`crates/qfs/src/claude.rs`) takes *"the first non-empty trimmed line"* of `claude --bg`'s stdout as
the new session id. Claude Code 2.1.217 actually prints:

```
Starting background service…
backgrounded · eb5300ad
  claude agents             list sessions
  ...
```

So the captured "id" is the literal string `Starting background service…`, not the session id. The
launch still *succeeds* (the row is real — `claude agents --json` and the qfs sessions relation both
show the true `sessionId` `eb5300ad-f06d-45fb-8082-e7a9948af1f6`), so the session is discoverable by
a follow-up read; but the `RETURNING id` the launch hands back is garbage, which breaks the
"launch → capture id → steer that id" composition the mission wants (the caller cannot address the
new session by the returned value).

## Expected

`RETURNING id` returns the launched session's real id (the `sessionId`, e.g.
`eb5300ad-f06d-45fb-8082-e7a9948af1f6`, or at least the short `eb5300ad` handle that
`claude attach`/`claude logs`/`claude stop` accept).

## Suggested fix

Two robust options (pick per the CLI contract):

1. Parse the `backgrounded · <shortid>` line rather than the first line, then (optionally) resolve
   the short id to the full `sessionId` via `claude agents --json` (which lists `{id, sessionId,
   pid, cwd, name, status}`); or
2. After the spawn, read the newest `sessions/<pid>.json` / `claude agents --json` entry whose `cwd`
   + `name` match the `LaunchSpec` and return its `sessionId`.

Option 2 also sidesteps any future change to the `--bg` banner text.

## Policies

- `workaholic:implementation` / honest-surfaces — a verb that returns an id must return the id a
  later read resolves to; a `RETURNING id` that names no addressable session is a dishonest surface
  (the launch reports success with a value the caller cannot use).
- `workaholic:design` / access control — the id-discovery reads only session METADATA the runtime
  already wrote (`claude agents --json` / `sessions/<pid>.json`); no transcript or credential
  crosses the seam.

## Quality Gate

- A launch's `RETURNING id` equals the id that the subsequent `/claude/sessions` read shows for the
  new session (proven hermetically with a fake launcher whose stdout mimics the real
  `Starting background service… / backgrounded · <id>` banner; the live re-proof rides the
  container live-round).
- The existing hermetic launcher tests (argv-as-data, bad-cwd refusal) stay green.

## Notes (live-fire evidence, 2026-07-22, in-container)

- `claude --bg '<prompt>'` is the correct invocation — the CLI itself rejects `--bg --print` with
  *"The prompt is the positional — drop --print: `claude --bg '<task>'`"*, exactly what the launcher
  builds. So the argv contract is right; only the id capture is wrong.
- `claude agents` needs a TTY; `claude agents --json` is the machine-readable listing to resolve ids.
