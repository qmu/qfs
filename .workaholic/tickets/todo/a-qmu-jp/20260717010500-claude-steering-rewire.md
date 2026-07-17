---
created_at: 2026-07-17T01:05:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: [20260717010100-claude-real-store-reader.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Steering rewire: the instructions INSERT must reach something a session reads

## Overview

Mission acceptance item 5 (owner capability 4: "send a message to a session — answering the
question it is blocked on"). The pre-mission append leg worked end-to-end mechanically but
wrote `<base>/<id>/instructions` — a file **no Claude Code session ever reads**. It steered
nothing. The real-store-reader ticket (20260717010100) replaced that with an honest fail-closed
error; this ticket turns steering back on, for real.

Design first — the medium is an open product decision the driver header always flagged. What
the real store offers (verify each against the running product before betting on it):

- `~/.claude/sessions/<pid>.json` records carry `peerProtocol: 1` — the session process speaks
  a local peer protocol (this is how one session's SendMessage reaches another). Find its
  transport (socket path / named pipe under `~/.claude`), speak it, or shell out to the
  sanctioned client if one exists.
- `claude` CLI surfaces: a `claude --resume <id> -p '<msg>'` non-interactive append is a
  legitimate but heavier fallback (it runs a turn, not just queues a message).
- The tasks/queue surfaces under `~/.claude` (`tasks/`, `queue-operation` transcript entries)
  may expose an inbox a running session drains — verify, do not assume.

Rule (mission): an INSERT into `/hosts/<host>/claude/sessions/<id>/instructions` is **observed
by the target session**. If after investigation no medium exists that a session actually reads,
record that as the honest design decision in the blueprint and keep the surface fail-closed —
do NOT resurrect a write-only append log to make the verb "work".

The `SessionSource::append_instruction` seam and the `ClaudeApplier`/capability gates
(INSERT-only, no UPDATE/REMOVE) stay; only the sink behind the seam changes.
`scan_instructions` should read back whatever medium is chosen (the append-log read face stays
truthful).

## Policies

- `workaholic:design` / access control — steering a session is acting as the operator; the
  write stays behind the explicit COMMIT gate, reversible-append semantics only, unknown ids
  fail closed (`claude session <id> not found` behaviour preserved).
- `workaholic:implementation` / honest-surfaces — a verb that cannot reach a live session does
  not pretend to; fail-closed beats write-only.

## Quality Gate

1. Live proof (recorded in the mission changelog): an INSERT steers a real target session and
   the steered text observably arrives in that session's transcript/behaviour.
2. Hermetic tests over a fake medium behind the seam: append routes to the chosen transport,
   unknown session fails closed, UPDATE/REMOVE still structurally rejected.
3. If the outcome is "no readable medium exists": the blueprint records it, the surface stays
   fail-closed, and this ticket closes with that decision — explicitly, not silently.

## Considerations

- Steering another user's session is out of scope: same-user, same-host only (the record's
  uid/dir ownership is the boundary).
- The e2e must not steer the CI runner's own driving session into chaos — target a scratch
  session the test launches (couples to the CREATE ticket 20260717010600 if launch lands
  first; otherwise use a mock).

## Investigation record (2026-07-17 — ticket NOT closed; surface stays fail-closed)

Investigated on the real box during the canon drive (work-20260717-101005); the ticket stays in
todo because the decisive probes need an owner-attended session:

- **The peer protocol exists.** Every `~/.claude/sessions/<pid>.json` record carries
  `peerProtocol: 1` (verified on a live record) — sessions do speak a local peer transport
  (this is how one session's SendMessage reaches another).
- **Candidate inboxes located, none verifiable from here.** `~/.claude/daemon/` (control.key,
  dispatch/, roster.json — a dispatch surface with an auth key), `~/.claude/teams/session-*/`,
  and `~/.claude/tasks/<uuid>/` all exist. This session's tool-permission classifier BLOCKED
  every deeper probe (socket scan, /proc fd inspection, team/task dir reads), so the transport
  and inbox formats remain unverified — "no verifiable medium from this session", which is NOT
  the same finding as "no medium exists".
- **No sanctioned public client.** `claude --help` / `claude agents --help` expose list/dispatch
  surfaces (`--bg`, `claude agents --json`) but no send-to-session verb. `claude --resume <id>
  -p '<msg>'` remains the ticket's named heavier fallback: it RUNS A TURN on the transcript
  (spend + a race against the live process holding the session), it does not queue a message —
  ruling it in or out is an owner call.
- Per this ticket's own rule, the append was NOT resurrected as a write-only log:
  `ClaudeStoreSource::append_instruction` keeps failing closed naming this ticket.

Next step (owner-attended): probe the daemon dispatch / teams inbox formats from an
unrestricted shell, or rule the `--resume -p` fallback; then wire the chosen medium behind the
unchanged `SessionSource::append_instruction` seam with `scan_instructions` reading it back.
