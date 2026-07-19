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

## Owner decision (2026-07-18 — replan: medium probe is owner-attended)

Ruled in a `/monitor` replan (AskUserQuestion, 2026-07-18): **the transport medium is chosen by
an owner-attended probe**, not the `--resume -p` turn-running fallback and not fail-closed
retirement. The ticket stays in todo, fail-closed, until that probe picks a medium. This is a
deliberate escalation-block: the decisive reads need an unrestricted shell the autonomous session
does not have.

Further evidence gathered this replan (from the reads that WERE permitted here):

- `~/.claude/daemon/roster.json` is readable and confirms the peer transport concretely: each
  live worker record carries a `rendezvousSock` and a `ptySock` under `/tmp/cc-daemon-1000/
  <supervisor>/{rv,pty}/<short>.sock`, plus per-worker `rvAuth` / `ptyAuth` tokens and a
  daemon-level `control.key` under `~/.claude/daemon/`. So the medium is a **per-session unix
  socket authenticated by a token the roster hands out** — the most promising sink for the
  instructions append, ahead of the teams/tasks dirs.
- But the socket directory `/tmp/cc-daemon-1000/` did not exist at probe time (the supervisor pid
  in the roster may be stale), and reads under `~/.claude/teams/session-*/` were **denied by this
  session's tool-permission classifier**. So the rendezvous/pty framing on the wire stays
  unverified from here — exactly the "no verifiable medium from THIS session" boundary, which is
  why the owner-attended probe is the ruled next action.

Concretely, the owner-attended probe should: (1) confirm a live `rendezvousSock`/`ptySock` exists
for a running session, (2) read the `rvAuth`/`ptyAuth` framing (or the `control.key` dispatch
protocol under `~/.claude/daemon/dispatch/`), and (3) decide whether the instructions append
speaks that socket directly or shells out to a sanctioned client — then wire it behind the
unchanged `SessionSource::append_instruction` seam.

## Owner-attended probe result (2026-07-19 — medium identified: the teams inbox)

The probe ran in the owner's unrestricted terminal (`/monitor`, 2026-07-19). Findings:

- **The rendezvous/pty sockets are NOT a usable sink.** `roster.json` still lists a worker
  (`a6239df4`) with `rendezvousSock`/`ptySock` paths under `/tmp/cc-daemon-1000/<supervisor>/`,
  but that directory **does not exist** — `/tmp` is tmpfs and clears on reboot, so the roster
  record is stale and the socket transport is gone. A file under `/tmp` cannot be the durable
  steering sink (this confirms the original worry).
- **The medium is the teams inbox.** `~/.claude/teams/<session>/inboxes/<recipient>.json` — one
  JSON file **per recipient** (observed live: `drive-jishakabu.json`, `team-lead.json`, filenames
  = role/member names), each a **JSON array of messages** that the running session drains (the
  files sit at `[]` once drained). This is the concrete realisation of the `peerProtocol: 1`
  peer transport: a SendMessage to a teammate appends a message object to that recipient's inbox
  array. `~/.claude/daemon/dispatch/` is empty and `claude agents` exposes only launch/list
  (`--json`, `--agent`, `--model`, …) — no send-to-session verb — so the inbox file IS the sink,
  not a CLI shell-out.

**Decision (owner, 2026-07-19):** wire steering as an **append to the target's teams inbox JSON
array** behind the unchanged `SessionSource::append_instruction` seam; `scan_instructions` reads
the same array back. Same-user/same-host only (the inbox dir's uid/ownership is the boundary);
unknown session still fails closed.

**Refinement (same probe):** the inbox is keyed by **member name**, and it is a **team
construct**. `config.json` carries `{createdAt, leadAgentId, leadSessionId, members, name}`; each
member is `{agentId: "<name>@session-<id>", name, agentType, joinedAt, tmuxPaneId, cwd,
subscriptions, backendType}`, and `inboxes/<member-name>.json` is that member's queue. So this
medium steers a **team session's member**; a standalone (non-team) session may have no inbox dir
at all — the implementation must branch on whether the target session is a team member, and rule
what steering a plain single session means (out of scope here, or a different sink). The
session-id → inbox mapping is `member.agentId` = `<name>@session-<id>`, so the sessions relation's
`id` resolves to the `session-<id>` half.

**One detail remains for implementation:** the exact message-object schema (the keys in an inbox
array element — `from`/`to`/`text`/`id`/`ts` or similar). Every observed inbox was empty (`[]`),
so the shape must be captured from **one real message in flight** (snapshot a live session's
`inboxes/<recipient>.json` the moment a message arrives) — an owner-attended one-liner at
implementation time — or read from the CLI's own inbox-writer. Until that single capture, the
append target (the file + JSON-array format) is known; only the element's field names are not.
The surface therefore stays fail-closed until the schema is captured and the append is wired and
hermetically tested (fake inbox dir behind the seam; unknown session fails closed; UPDATE/REMOVE
still structurally rejected).

## Replan (2026-07-19 — pty/tmux transport RETIRED; teams inbox is the canonical, non-process-killing sink)

Owner ruling, 2026-07-19: the earlier **"(A) qfs-owned pty/tmux" / rendezvous-socket transport is
RETIRED as unsafe** and must not be pursued. It is process-coupled — speaking a live session's
pty/rendezvous socket reaches *into* a running process, and exercising it on this shared host
(which runs the owner's live Claude Code sessions) repeatedly crashed the parent session. This
supersedes every "the socket is the most promising sink" line in the investigation records above;
treat those as historical, not the plan.

**The canonical design is the teams inbox — a durable queue, delivered without touching any
process.** Wire `SessionSource::append_instruction` (seam unchanged) to **append one message object
to the target session's teams-inbox JSON array** — `~/.claude/teams/<session>/inboxes/<member>.json`
(a per-recipient array the running session drains on its own schedule). This is a plain durable-file
enqueue: qfs writes a message and returns; it spawns nothing, signals nothing, and kills nothing.
The target reads the message when it next drains its inbox. `scan_instructions` reads the same array
back (the read face stays truthful). Session-id → inbox mapping is `member.agentId = <name>@session-<id>`
(the sessions relation's `id` is the `session-<id>` half); same-user/same-host only (inbox-dir
ownership is the boundary); unknown session / non-team session with no inbox fails closed.

**Design decisions to settle at implementation (all within the inbox model — no return to sockets):**

- **The message-object schema.** Still the one unknown: every observed inbox was `[]`, so the element
  field names (`from`/`to`/`text`/`id`/`ts` or similar) must be captured from one real in-flight
  message, or read from the CLI's own inbox-writer. Until captured, the surface stays fail-closed.
- **Non-team (standalone) sessions.** A plain single session may have no `inboxes/` dir. Rule at
  implementation whether steering such a session is out of scope (fail closed, "no inbox for a
  non-team session") or gets a different durable sink — but never a process-coupled one.

**Environment gate — this ticket's DRIVE runs ONLY in an isolated environment (owner ruling).**
The hermetic slice — the append behind a *fake* inbox dir, unknown-session fail-closed, UPDATE/REMOVE
still rejected, `scan_instructions` round-trip — may be authored anywhere and is pure file I/O over a
fixture. But the two steps that touch real sessions — **(a) the one-shot inbox message-schema capture
from a live session, and (b) QG1's live-fire proof that a real target session observes the steered
text** — MUST run in an isolated box (a container/VM with no live Claude Code sessions), never this
shared host, and are **out of unattended / `/monitor` scope**. The ticket stays in `todo/`, NOT
drive-authorized: its live proof is parked for an isolated/attended session even though the transport
question is now closed.
