---
created_at: 2026-07-17T01:03:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: 9954c77babb8edf7c26680dedc55bf38446c0db1
category: Added
depends_on: [20260717010200-claude-mount-registration-and-e2e-guard.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Serve the sessions query over HTTP — the mission's live-app gate surface

## Overview

The mission gate (`gate_type: live-app`, `gate_target: /claude/sessions`): an HTTP endpoint
bound over the sessions query, served on the worktree's dev port, returns one row per live
Claude Code session read from the real store — including the session driving the check — each
with a non-empty `last_message`.

`qfs serve <config.qfs>` already runs `create endpoint <name> on 'GET /<route>' as <pipeline>`
bindings (docs/server.md), but the serve composition root builds its own engine with only the
`/status` builtin + `/server` face (`packages/qfs/crates/qfs/src/serve.rs:27-42`,
`serve_builtins.rs`) — a `/claude/sessions` endpoint would 422 as an unregistered source even
after the shell-side mount fix, because serve never registers the claude mount or read facet.

Ship:

1. serve-side registration in `run_serve`: the `ClaudeDriver` mount + (when
   `QFS_CLAUDE_SESSIONS` resolves a source) the `ClaudeReadDriver` read facet, mirroring the
   shell wiring — one composition per face, same fail-closed default;
2. a hermetic serve e2e: boot `qfs serve` with a FIXTURE store + a config carrying
   `create endpoint sessions on 'GET /sessions' as /claude/sessions`, fetch it, assert one row
   per fixture live session with non-empty `last_message`;
3. the live gate run (operator/driver action, not a committed test): serve with
   `QFS_CLAUDE_SESSIONS=$HOME/.claude` and `QFS_HTTP_ADDR=127.0.0.1:<dev-port>` on this
   worktree, curl once, record the result in the mission changelog. The gate can only pass live
   against the real store — hermetic tests do not tick it.

## Policies

- `workaholic:design` / access control — the bind stays loopback-default (`DEFAULT_BIND_ADDR`,
  blueprint §8); exposure of real transcripts' `last_message` over the local dev port was
  explicitly approved by the owner (AskUserQuestion, 2026-07-17). No non-loopback bind is part
  of this ticket.
- `workaholic:operation` / runtime — the probe server is torn down after the check; nothing
  long-running ships from this ticket.
- `workaholic:implementation` / test — the committed test uses the fixture store; only the
  operator-run gate probe reads the real `~/.claude`.

## Quality Gate

1. The hermetic serve e2e passes: endpoint over `/claude/sessions` returns fixture rows as JSON.
2. Without `QFS_CLAUDE_SESSIONS`, the same endpoint yields the structured 422 (fail-closed).
3. Live probe (recorded, not committed): the endpoint on the dev port returns ≥1 row including
   the driving session's id with non-empty `last_message`.
4. Baseline gates: workspace tests, clippy `-D warnings`, fmt, gen-docs `--check`.

## Considerations

- This repo has no per-worktree dev-port table; the qfs HTTP default is `127.0.0.1:8787`
  (`crates/http/src/serve.rs:35`). Use it as this worktree's dev port unless taken; record the
  actual port with the gate evidence.
- The mission's canonical path is `/hosts/<host>/claude/...`; until the canon ticket lands the
  endpoint binds over the mount path `/claude/sessions`. When the canon moves, the gate config
  moves with it — keep the endpoint config in the mission record, not baked into serve.
