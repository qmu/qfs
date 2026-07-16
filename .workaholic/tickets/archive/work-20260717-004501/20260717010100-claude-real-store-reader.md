---
created_at: 2026-07-17T01:01:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: 9954c77babb8edf7c26680dedc55bf38446c0db1
category: Changed
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Real Claude Code store reader behind the unchanged SessionSource seam

## Overview

Mission acceptance item 3. `DirSessionSource` (`packages/qfs/crates/qfs/src/claude.rs:54-177`)
reads a hand-invented `<base>/<id>/meta` + `<base>/<id>/instructions` layout that exists nowhere
on any machine — pointing it at the real store yields zero rows. Claude Code actually writes
(verified on this box, 2026-07-17):

- **`~/.claude/sessions/<pid>.json`** — one JSON record per session process: `pid`, `sessionId`,
  `cwd`, `name`, `status` (`busy`/…), `kind`, `startedAt`, `updatedAt`. This is the store's own
  liveness registry (37 records on this box; the session driving this mission is
  `~/.claude/sessions/3262249.json`).
- **`~/.claude/projects/<slugified-cwd>/<uuid>.jsonl`** — the transcript: one JSON entry per
  line; message entries carry `type` (`user`/`assistant`), `message.content` (a string or an
  array of typed blocks), `timestamp`, `cwd`, `sessionId`. Non-message entry types
  (`file-history-snapshot`, `queue-operation`, `pr-link`, `system`, …) interleave freely.
  Slugification: every character outside `[A-Za-z0-9-]` becomes `-` (verified:
  `/home/ec2-user/projects/data-platform/.worktrees/ai-letter` →
  `-home-ec2-user-projects-data-platform--worktrees-ai-letter`).

Replace `DirSessionSource` with a `ClaudeStoreSource` over the real store, behind the
**unchanged** `SessionSource` trait (`driver-claude/src/backend.rs:26-57`) — proving the seam
claim the mission makes. `QFS_CLAUDE_SESSIONS` now names the Claude home dir (e.g.
`~/.claude`); unset stays fail-closed (no source, `/claude` unwired), preserving the opt-in
stance.

Row construction:
- one row per `sessions/*.json` record whose process is alive (liveness heuristic: the record
  is what the store offers; confirm with `/proc/<pid>` existence on Linux, degrade to
  record-presence where `/proc` is absent);
- `id` = `sessionId`, `cwd` = record `cwd`, `name` = record `name` (nullable), `status` =
  record `status` else `"unknown"`;
- `last_message` = the last transcript entry (read the file tail, iterate lines backwards)
  of type `user`/`assistant` whose `message.content` yields non-empty text (string content
  as-is; array content = the concatenated `type=="text"` blocks — `tool_use`/`tool_result`
  blocks never surface), capped at a documented length.

The sessions schema (`driver-claude/src/schema.rs:107-125`) changes to what the store truthfully
offers: `id`/`cwd`/`name`/`status`/`last_message` — `task` and `progress` are dropped (no such
fields exist in the real store; a permanently-Null column is a fiction). The structural
redaction test extends to the new shape. `gen-docs` re-renders `docs/drivers.md`.

Steering interim (until the rewire ticket): `append_instruction` **fails closed** with a
structured error naming the rewire ticket — the old append wrote a file no session reads, and an
append that steers nothing is worse than an honest refusal. `scan_instructions` returns an empty
batch. The `ClaudeApplier` and its capability gates stay untouched.

## Policies

- `workaholic:implementation` / honest-surfaces — a queryable path answers from real state or
  fails closed; it never fabricates (the invented layout was exactly this failure).
- `workaholic:design` / data-access — the reader surfaces session metadata + last message text
  only; no token, key, or raw-transcript column exists in the schema (structural redaction
  holds). Developer exposure of real transcripts' `last_message` was explicitly approved by
  the owner (AskUserQuestion, 2026-07-17).
- `workaholic:implementation` / test — hermetic: unit tests build a FIXTURE store in a tempdir
  (fixture `sessions/*.json` + `projects/<slug>/<uuid>.jsonl`); no test reads the developer's
  real `~/.claude`.

## Quality Gate

1. Unit tests over a tempdir fixture store: a live-pid record surfaces a row with the right
   `id`/`cwd`/`status` and the transcript's final text as `last_message`; a dead-pid record is
   filtered; a record with no transcript yields a Null `last_message` (row still present); a
   transcript whose tail is tool-traffic walks back to the last real text.
2. Schema drift guard: the scanned batch schema equals `claude_node_schema(Sessions)` exactly.
3. `append_instruction` fails closed with the structured rewire error; `scan_instructions` is
   empty; the driver-crate capability gates stay green.
4. Baseline gates: workspace tests, clippy `-D warnings`, fmt, `gen-docs --check` after
   regeneration.

## Considerations

- The slug function must round-trip the observed store, not an assumed spec — test against the
  verified examples above.
- Transcript tails can be large; read a bounded tail chunk (e.g. 256 KiB), not the whole file.
- Whether `QFS_CLAUDE_SESSIONS` should default to `~/.claude` when present (capability 1
  "how many sessions are running" with zero config) is an owner call — record it when taken;
  this ticket keeps opt-in.
- A session record can outlive its process (crash); the pid check is the honest filter. Pid
  reuse is theoretically possible — `procStart` is available in the record if a stricter check
  is ever needed.
