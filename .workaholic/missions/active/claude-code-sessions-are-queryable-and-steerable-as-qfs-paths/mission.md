---
type: Mission
title: Claude Code sessions are queryable and steerable as qfs paths
slug: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
status: active
created_at: 2026-07-16T01:50:14+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: ai-sessions-are-ordinary-qfs-surfaces
drive_authorized: true
tickets:
  - 20260717010100-claude-real-store-reader.md
  - 20260717010200-claude-mount-registration-and-e2e-guard.md
  - 20260717010300-claude-gate-endpoint-on-serve.md
  - 20260717010400-claude-path-canon-hosts-move.md
  - 20260717010500-claude-steering-rewire.md
  - 20260717010600-claude-session-create-launch.md
  - 20260717010700-claude-compiled-standing-recorded.md
  - 20260719231005-claude-live-round-owner-attended.md
  - 20260719105527-tmux-session-teardown-must-not-kill-server.md
  - 20260721190756-slack-driver-channel-id-resolution.md
stories: []
concerns: []
gate_type: live-app
gate_target: /claude/sessions
gate_assert: An HTTP endpoint bound over the sessions query, served on the worktree's dev port, returns one row per live Claude Code session read from the real on-disk store — including the session driving the check — each carrying a non-empty last_message.
---

# Claude Code sessions are queryable and steerable as qfs paths

## Goal

**Anything you can do at a Claude Code session, you can do with a qfs query** (owner directive,
2026-07-16). A machine's running agents are not a special case reachable only by sitting at a
terminal — they are ordinary qfs paths: you point qfs at the local Claude Code store and ask how
many sessions are running, what each is doing, what the latest message was, and you steer one by
writing to it. The concrete surface the owner named:

1. how many Claude Code sessions are running,
2. launch another session,
3. the latest message on a session,
4. send a message to a session — answering the question it is blocked on.

This is a **standing product property**, not a build: "everything is a path" either extends to the
agents on the box or it does not. Today it does not, and the interesting part is *why*.

**This mission exists because the feature is ~75% built and 0% useful.** Every claim below was
verified against the source and the shipped `v0.0.71` binary on 2026-07-16 — not inferred from the
crate's doc comments, which are accurate in some places and dangling in others.

**What is true today.** `qfs-driver-claude` ships a well-factored driver: the pure/impure split
mirrors `/sys` (introspective half credential-free at `driver-claude/src/lib.rs:100-146`; a
`NoopApplier` as the trait-slot filler at `:151-160`). The `SessionSource` seam
(`backend.rs:26-57`), the `ClaudeApplier` (`applier.rs:30-103`), a real on-disk `DirSessionSource`
(`qfs/src/claude.rs:54-177`, genuine `std::fs`), and the applier registration
(`commit.rs:326-333`) all exist. **Steering works end-to-end** — an INSERT into
`/claude/sessions/<id>/instructions` commits and appends, and steering an unknown id fails closed
with `claude session <id> not found`. UPDATE/REMOVE are structurally rejected twice
(`lib.rs:91-98`, `applier.rs:62-66`).

**What is not true yet**, each verified:

1. **The read path is unreachable.** `/claude/sessions` returns `unknown_source`. `/local`, `/sql`,
   `/git`, `/sys`, `/transform` and `/type` each get an `engine.mounts.register(...)` in
   `qfs/src/shell.rs` (`:139`, `:259`, `:264`, `:273`, `:288`, `:312`). `/claude` gets **only** the
   read facet (`:328`) and **never a mount**. `unknown_source` is raised by
   `pushdown/src/planner.rs:151` against the *mount* registry, so no env var can reach it. It is a
   one-line omission.
2. **A passing test pins the bug as correct.** `e2e_cli.rs:356-367` asserts `/claude` →
   `unknown_source` as the intended "no read driver registered" case. It passes **for the wrong
   reason**, and it is why nothing caught this. No test sets `QFS_CLAUDE_SESSIONS`; `claude.rs`'s
   own tests call `DirSessionSource` directly, never through the engine.
3. **The on-disk format is fictional.** `DirSessionSource` reads a hand-invented layout
   (`claude.rs:26-35`): `<base>/<id>/meta` as `key=value` lines plus `<base>/<id>/instructions`.
   Claude Code actually writes `~/.claude/projects/<slugified-cwd>/<uuid>.jsonl`. Verified on this
   box: **zero `meta` files exist; the `.jsonl` transcripts do.** So pointing the driver at the real
   store yields zero rows — and **the working append writes into a file no session ever reads.**
   It steers nothing. The driver's own header flagged this honestly as an open product decision
   ("named here rather than baked into the driver crate"), and t64
   (`20260626102200-t64-claude-driver.md:104-106`) flagged it too. **No ticket ever resolved it**;
   the placeholder silently became the answer.
4. **Launching a session does not exist.** No spawn, no CREATE, no `Command::new`; `Capabilities`
   are Select-only. t64 pre-ruled only *teardown* (irreversible `Remove`). Create was never
   specified — this is greenfield with no design behind it.
5. **The path canon contradicts itself.** t64's own title ruled `/hosts/<host>/claude/...` and said
   it *"supersedes the old top-level `/claude/...`"*; the code shipped top-level `/claude`
   (`schema.rs:30`). Never reconciled. **Ruled 2026-07-16 (owner): `/hosts/<host>/claude/...` is
   canonical and top-level `/claude` retires.** qfs is experimental — a hard break is correct.
6. **The crate's roadmap citation is dangling.** `lib.rs:1` cites "roadmap §3.3 / M7, t64".
   `docs/roadmap.md` is 153 lines with **no numbered sections, no M7, no t64, and zero mentions of
   `/claude`** — it points at a superseded roadmap that no longer exists in this repo. `/claude`
   carries no ✅/🧭 marker anywhere.

**Why the docs did not catch any of this.** `DESCRIBE /claude/sessions` works and `docs/drivers.md`
lists `/claude`, because both read the *separate* `compiled_describe_registry` (`describe.rs:283`),
which never touches the mount registry. The generated docs are **true about the schema and false
about reachability** — which is also why this mission's gate is `live-app` and not `documentation`:
a docs gate here would pass today, against a driver that cannot be read.

## Scope

**Done when** every acceptance item is ticked: the path canon is ruled in the blueprint, the read
path is mounted and covered by a test that fails when it regresses, a real reader answers from
Claude Code's actual store, the four named capabilities are answerable and steerable through a qfs
query, and `/claude`'s standing as a compiled driver is recorded rather than left unnamed.

**Out of scope:**

- **Agents as principals.** The `support-create-agent-semantics-…` mission rules an agent as *"a
  principal, not a process"* — daemon-local, launched by the cron sweeper firing a saved query plan
  under its own identity. `/claude` is the opposite axis: a façade over a runtime that lives
  elsewhere, running as the operator, with no identity, grant, or audit dimension of its own. That
  mission's six acceptance items contain zero occurrences of "claude", "session" or "spawn"
  (verified). Kept separate deliberately (owner decision, 2026-07-16). If the two ever unify — a
  `CREATE AGENT` principal that *is* a Claude Code session — that is a later re-ruling of the agent
  model, not this mission's work.
- **Converting `/claude` into a declared driver.** It is a compiled-Rust driver and therefore an
  unnamed counter-example to the `declared-drivers-…` mission's absolute rule ("never a
  compiled-Rust driver", which today names only `/cf`). But the declared shape is REST-shaped
  (`base_url`/`auth`/`pagination`/`verb`/`body`) and `/claude` has no base URL, no auth, and no
  wire — it **mechanically cannot be declared today**. Blueprint §13 (`blueprint.md:915-917`) frames
  this as a **ratchet, not a partition**: "Compiled drivers remain until their script twin passes
  the conformance suite." So `/claude` is not in violation; the ratchet has not reached it. This
  mission only makes that standing explicit (item 6); raising the ratchet belongs to the declared
  driver work.
- **The cross-machine tunnel hop.** `/hosts/<host>/claude/...` reaching *another* box rides the t63
  tunnel and re-checks POLICY at the destination — a documented seam, not wired
  (`claude.rs:22-24`). This mission rules the path shape and serves the local host; the remote hop
  is later work.
- Full Claude Code feature parity. The Goal's "anything you can do" is the **direction**; the four
  named capabilities are this mission's measurable surface.

**Environment constraint — items 5 and 6 exercise real OS processes, so they run ONLY in an
isolated environment (owner ruling, 2026-07-19).** Steering and launch touch live processes:
launch spawns `claude` processes, and any exercise of steering or launch against real sessions can
crash co-resident live Claude Code sessions. The earlier **"(A) qfs-owned pty/tmux" steering/launch
transport is RETIRED as unsafe** — driving it on this shared host, which runs the owner's live
sessions, repeatedly crashed the parent session. Steering's canonical medium is now the **teams
inbox**: a durable per-recipient JSON queue the target session drains, so a message is *delivered*
without spawning or killing any process (see item 5). The hermetic implementation of both items may
be authored anywhere, but every step that exercises real processes — the steering live fire, the
inbox message-schema capture, a real `--bg` launch, the composed launch→steer proof — MUST run in
an isolated box (a container/VM with no live sessions), never the shared host.

**Container ruling (owner, 2026-07-22): a container on this host qualifies as the isolated box.**
The host's `docker` command is podman 5.8.4. Every process-exercising step — the steering live
fire, the inbox message-schema capture, a real `--bg` launch, the composed launch→steer proof,
and any tmux server a test spawns — runs **inside a container** with no live sessions: never
mount the host's `~/.claude`, never inherit the host's tmux sockets or `TMUX_TMPDIR`. Under that
constraint the remaining set IS unattended-drivable, so `drive_authorized` is stamped for the
overnight run; anything that cannot be containerized escalates instead of running on the shared
host.

## Experience

What must be observable when this mission is achieved (added 2026-07-22 when the schema began
requiring it; restates the Goal's owner-named capabilities, no new scope):

- An operator (or agent) asks a machine "how many sessions are running, and what did each last
  say" with one qfs query over `/hosts/<host>/claude/sessions` and gets one truthful row per
  live session, read from the real on-disk store — never an invented layout.
- Steering is a query: an INSERT into `.../sessions/<id>/instructions` appends to the target
  session's durable teams inbox and is observed by that session — delivered without spawning
  or killing any process. A write that nothing would read fails closed instead of pretending.
- Launching is a query: an INSERT into the sessions set starts a new session behind the
  irreversible gate, and the new session's id is addressable by the same paths immediately.
- Bare `/claude` paths answer with the canonical `/hosts/<host>/claude/...` spelling in the
  error, so the retired shape teaches the current one.
- Every process-exercising proof runs in an isolated container, never on a shared host with
  live sessions (see the Scope environment constraint).

## Acceptance

- [x] **The path canon is ruled and implemented.** The blueprint records `/hosts/<host>/claude/...`
      as canonical and top-level `/claude` as retired, honouring t64's ruling that the code
      contradicted; `schema.rs:30` and `peel_scope` follow, and the dangling "roadmap §3.3 / M7,
      t64" citation at `lib.rs:1` is corrected against the roadmap that actually exists (owner
      ruling, 2026-07-16) *(done 2026-07-17: blueprint §8 records the ruling; the engine peels
      `/hosts/local/<svc>` generally in `resolve_path` + planning + the write evaluator +
      DESCRIBE; bare `/claude` fails `retired_path` naming the canonical form; a non-local host
      fails `remote_host_not_executable`; `docs/roadmap.md` gains the "AI sessions as paths"
      section with honest ✅/🧭 markers and `lib.rs:1` cites it)*
- [x] **The read path is mounted, and a test fails if it un-mounts.** `/claude/sessions` resolves
      instead of raising `unknown_source` — the missing `engine.mounts.register(...)` in
      `shell.rs` alongside `/sys` (`:273`) — and `e2e_cli.rs:356-367`, which today asserts the bug
      as intended behaviour and passes for the wrong reason, is corrected into a guard that would
      have caught it *(done 2026-07-17: mount registered unconditionally like `/sys`;
      `claude_sessions_reads_a_fixture_store_end_to_end` proven to fail when the registration is
      removed; the old test rewritten into planner- and read-registry-miss cases)*
- [x] **A real reader answers from Claude Code's actual store.** The invented `<base>/<id>/meta`
      layout (`claude.rs:26-35`) is replaced behind the **unchanged** `SessionSource` seam by a
      reader over `~/.claude/projects/<slugified-cwd>/<uuid>.jsonl`, including session-liveness
      detection (a transcript on disk is not a running session). The seam is correctly shaped to
      take this without touching the driver crate — that claim gets proven here *(done
      2026-07-17: `ClaudeStoreSource` joins the `~/.claude/sessions/<pid>.json` liveness registry
      — the store's own record of running processes, pid-checked against `/proc` — with the
      transcript tail. The SessionSource seam took it unchanged, as claimed; the driver crate's
      pure SCHEMA module was deliberately changed in the same slice — `task`/`progress`, which
      the real store does not record, gave way to `cwd`/`name`, which it does)*
- [x] **Session count and latest message are answerable by query.** A qfs query returns one row per
      running session with a truthful `last_message` (`schema.rs:117`), read from the real store —
      closing owner-named capabilities 1 and 3 *(done 2026-07-17: verified live over the gate
      endpoint — 37 rows = the 37 live registry records on this box, zero empty `last_message`)*
- [ ] **Steering reaches a real session via the teams inbox.** An INSERT into
      `/hosts/<host>/claude/sessions/<id>/instructions` is observed by the target session by
      **appending a message to that session's teams inbox** (`~/.claude/teams/<session>/inboxes/
      <member>.json` — a durable per-recipient JSON array the running session drains), rather than
      appending to a file nothing reads — closing owner-named capability 4, and turning the
      already-working write leg from a no-op into the feature. The retired pty/tmux/rendezvous-socket
      transport is deliberately NOT used: it is process-coupled and unsafe (owner ruling,
      2026-07-19). Non-process-killing by construction; the live-fire proof runs only in an isolated
      environment (see the Scope environment constraint)
- [ ] **Launching a session is designed, then shipped.** Greenfield: no grammar, no capability, no
      prior design. Needs a design brief first (what a launch *is*, whether it is irreversible and
      therefore gated, what identity it runs under, how its id becomes addressable) — closing
      owner-named capability 2 *(hermetic design + implementation done 2026-07-19, commit a73fa01 /
      v0.0.81: INSERT grammar, `Sessions` widened to Select+Insert, irreversible gate, launcher seam
      behind a fake, hermetic tests. The remaining live fire spawns a real `claude --bg` process, so
      it runs ONLY in an isolated environment, out of unattended scope — see the Scope environment
      constraint)*
- [x] **`/claude`'s compiled standing is recorded, not left unnamed.** The `declared-drivers-…`
      mission names `/claude` alongside `/cf` as a compiled counter-example, with blueprint §13's
      ratchet framing as what governs it — so the rule stops reading as absolute while two
      unnamed exceptions exist (the only "integration" between these missions that the evidence
      supports) *(done 2026-07-17: recorded in that mission's Goal section with the §13 ratchet
      quote and the mechanical why-not — no base URL, no auth, no wire; changelog line added
      there pointing back at this mission's item 7)*

## Changelog

- 2026-07-16 — Mission created on owner directive. Framed as a standing product property per the
  2026-07-15 reframing. Created **after** a full source-and-binary investigation rather than from
  the concern/doc record, because this repo had just had two mission acceptance items mis-stated in
  two days by paraphrase — the lesson applied here is that the crate's own header claims were
  partly accurate (the pure/impure split, the flagged format coupling) and partly dangling (the
  roadmap citation), and only reading the source separated them.
- 2026-07-16 — Two owner rulings taken before writing acceptance, because neither could be deferred
  without making the items unwritable: (1) **an independent mission**, not folded into
  `support-create-agent-semantics-…` (verified orthogonal: principal vs façade, zero term overlap)
  nor into `declared-drivers-…` (mechanically inexpressible in today's REST-shaped declaration);
  (2) **`/hosts/<host>/claude/...` is the canonical path**, retiring top-level `/claude` and
  honouring t64's ruling over the shipped code.
- 2026-07-16 — Gate set to `live-app` deliberately. A `documentation` gate would pass **today**
  against a driver that cannot be read, because `DESCRIBE` and `docs/drivers.md` render from
  `compiled_describe_registry` (`describe.rs:283`) and never touch the mount registry — the same
  blindness that let the unreachable read path ship. The gate drives an HTTP endpoint bound over
  the sessions query on the worktree's dev port, so it can only pass if the mount, the real reader,
  and the server surface are all true at once.
- 2026-07-17 — Ticket set decomposed (7 tickets, `todo/a-qmu-jp/20260717010100`–`010700`): real
  store reader → mount registration + e2e guard → gate endpoint on serve; path canon `/hosts`
  move; steering rewire; session CREATE/launch (design first); `/claude` compiled standing named
  in the declared-drivers mission. Ordered by `depends_on`. The developer explicitly approved
  (AskUserQuestion, 2026-07-17) exposing real session transcripts' `last_message` — including
  this driving session — through qfs queries and the local dev-port HTTP endpoint.
- 2026-07-17 — First slice landed (tickets 20260717010100/010200/010300): `ClaudeStoreSource`
  reads the REAL store (`~/.claude/sessions/<pid>.json` liveness registry joined with the
  `projects/<slug>/<uuid>.jsonl` transcript tail; sessions schema now truthfully
  `id`/`cwd`/`name`/`status`/`last_message`); the `/claude` mount registers unconditionally in
  shell + serve; the wrong-reason e2e became a regression guard proven to fail when the mount is
  removed; steering's append now FAILS CLOSED with a structured error naming the rewire ticket
  (the old append wrote a file no session read — honest refusal over a write-only no-op). Gates:
  workspace tests / clippy `-D warnings` / fmt / gen-docs / gen-skills / check-migrations all
  exit 0; version 0.0.73 → 0.0.74.
- 2026-07-17 — **Gate probe passed live**: `qfs serve` with
  `create endpoint sessions on 'GET /sessions' as /claude/sessions` on this worktree's dev port
  (`127.0.0.1:8794`; 8787 was occupied by an unrelated workerd) returned **37 rows = the 37 live
  registry records on this box**, INCLUDING the driving session
  (`7bd43a5c-edd8-49c4-8abb-5e543e70bfb5`, cwd `/home/ec2-user/projects/strategy`, status
  `busy`), with **zero empty `last_message`**. Server torn down after the check. Remaining for
  the mission gate proper: the canon ticket moves the bound path to `/hosts/<host>/claude/...`.
- 2026-07-17 — **Path canon shipped** (ticket 20260717010400, acceptance item 1 ticked):
  `/hosts/<host>/claude/...` is canonical, top-level `/claude` retired as a hard break. The
  general machinery: `MountRegistry::resolve_path` peels `/hosts/local/<svc>` via `peel_scope`
  (any mount, not a claude special-case); `require_host_realm(mount)` marks a mount's bare
  spelling retired; `canonicalize_host_path` enforces both and runs in read planning
  (`plan_pipeline`, incl. JOIN/subquery/set-op sources — the ScanNode carries the peeled service
  path), the write evaluator (`eval_write`), and DESCRIBE. Errors are structured and pointed:
  `retired_path` names the canonical form, `remote_host_not_executable` names the local remedy
  (the `require_known_host` precedent; the t63 tunnel hop stays out of scope). Blueprint §8
  records the ruling; `docs/roadmap.md` gains "AI sessions as paths" (✅ read surface, 🧭
  steering/launch/remote) and `lib.rs:1`'s dangling "roadmap §3.3 / M7" citation now points at
  it; gen-docs renders the catalog entry as `/hosts/<host>/claude` with example
  `/hosts/local/claude/sessions`; grep proves no doc teaches a bare `/claude/...` path. The
  serve-endpoint e2e binds `as /hosts/local/claude/sessions`; new e2e pins the retirement error,
  the remote-host refusal, and the canonical read over a fixture store. No skill teaches any
  `/claude` surface (cookbook grep clean), so no plugin version bump — the binary patch bumps
  0.0.74 → 0.0.75. Gates: workspace tests / clippy `-D warnings` / fmt / gen-docs / gen-skills /
  check-migrations all exit 0.
- 2026-07-17 — **Steering investigation recorded; the surface stays fail-closed** (ticket
  20260717010500 NOT closed — findings appended to the ticket). Verified on this box: the
  liveness registry records `peerProtocol: 1`, so a session-to-session peer transport exists in
  the product, and `~/.claude/daemon/` (control.key, dispatch/, roster.json) plus `~/.claude/
  teams/session-*/` and `~/.claude/tasks/<uuid>/` are candidate inboxes — but this session's
  permission boundary blocked probing any of them (socket scan, team/task dir reads all denied
  by the tool-permission classifier), and the public CLI surface exposes no send-to-session verb
  (`claude --help` / `claude agents --help`: list/dispatch only). Per the ticket's own rule, the
  append is NOT resurrected as a write-only log; it keeps failing closed naming the rewire
  ticket. Next step is owner-attended: probe the peer transport / teams inbox from an
  unrestricted shell, or rule the `claude --resume <id> -p` turn-running fallback in or out.
- 2026-07-17 — **Session-launch design brief written, awaiting owner acknowledgment** (ticket
  20260717010600 NOT implemented — its QG requires the owner's ruling before code):
  `design-brief-session-launch.md` beside this mission. Recommends: a launch is `claude --bg`
  (persistent background session, not a one-shot turn); the grammar is an INSERT into the
  sessions relation (blueprint §3 "creating a resource is INSERT INTO its collection" — no
  `CREATE SESSION` noun), widening `Sessions` capabilities to `Select+Insert` deliberately;
  the launch is irreversible-gated (spends money, starts an autonomous actor); identity is the
  operator (the agents-as-principals mission stays a separate axis); the id returns via
  `RETURNING` and appears in the sessions relation on boot. Three open questions put to the
  owner (grammar veto, irreversible gate, a `name` column).
- 2026-07-17 — ticket archived — 20260717010400-claude-path-canon-hosts-move.md
- 2026-07-17 — ticket archived — 20260717010700-claude-compiled-standing-recorded.md
- 2026-07-17 — **Canon proven live against the real store** (post-archive check, binary at
  0b60908): `QFS_CLAUDE_SESSIONS=~/.claude qfs run -e '/hosts/local/claude/sessions |> SELECT
  id, status |> LIMIT 3' --json` returned real live-session rows — including the driving
  session (`19b46573-…`, status `busy`) — exit 0; the bare `/claude/sessions |> LIMIT 1`
  returned `{"code":"retired_path","kind":"capability"}` naming
  `/hosts/local/claude/sessions`, exit 3.
- 2026-07-18 — **Replan: the two open owner decisions blocking items 5–6 were ruled**
  (`/monitor` interrogation, AskUserQuestion). Four rulings recorded durably in the tickets and
  the launch brief:
  1. **Steering medium (item 5) — owner-attended probe.** Not the `--resume -p` turn fallback,
     not fail-closed retirement: the transport is chosen by an owner-attended probe from an
     unrestricted shell. New evidence this replan — `~/.claude/daemon/roster.json` is readable
     and shows each live worker carrying a `rendezvousSock`/`ptySock` under `/tmp/cc-daemon-1000`
     with per-worker `rvAuth`/`ptyAuth` tokens and a daemon `control.key`, so the medium is a
     token-authenticated per-session unix socket (ahead of the teams/tasks dirs as the sink). The
     socket dir was absent at probe time and `teams/session-*` reads were classifier-denied, so
     the wire framing stays unverified from here. **Item 5 remains fail-closed, escalation-blocked
     on the owner-attended probe.**
  2–4. **Launch (item 6) — design acknowledged.** INSERT grammar (`Sessions` → `Select+Insert`,
     `RETURNING id`); irreversible-gated (`--commit-irreversible`/ack); optional `name` column
     accepted. **Quality gate 1 (owner acknowledgment before implementing commits) is satisfied.**
     The hermetic implementation (QG3) is now drive-ready; QG2's live proof stays owner-attended
     because it composes with the still-blocked steering.
  No tickets emitted and no acceptance items added or reworded — this replan resolved round-4
  per-ticket decisions on the existing tickets 010500/010600. `drive_authorized` **not** stamped:
  the steering medium is deferred to owner-attended work, so the remaining set is not uniformly
  drive-ready and both remaining live proofs need an owner-attended session (real Claude spend /
  classifier-blocked daemon probing).
- 2026-07-18 — mission replanned — design-brief-session-launch.md
- 2026-07-19 — **Replan: the pty/tmux steering transport is RETIRED as unsafe; the teams inbox is
  canonical (owner ruling, 2026-07-19).** The earlier candidate transport — the qfs-owned
  pty/tmux / rendezvous-socket medium the 2026-07-18 changelog leaned toward — is retired: it is
  process-coupled, and driving the steering/launch legs on this shared host (which runs the owner's
  live Claude Code sessions) repeatedly crashed the parent session. Steering's canonical medium is
  now the **teams inbox** identified by the owner-attended probe (commit 89fd431):
  `~/.claude/teams/<session>/inboxes/<member>.json`, a durable per-recipient JSON array the running
  session drains — a message is *delivered*, not injected into a process, so steering spawns and
  kills nothing. Recorded a **Scope environment constraint**: items 5 (steering) and 6 (launch)
  exercise real OS processes, so every step that touches real sessions (steering's live fire, the
  inbox message-schema capture, a real `--bg` launch, the composed proof) runs ONLY in an isolated
  box with no live sessions — never the shared host — and is out of unattended / `/monitor` scope.
  Acceptance item 5 reworded to name the teams-inbox medium and the retirement; item 6 annotated
  with the isolated-env constraint on its remaining live fire. Delta tickets 20260717010500
  (steering) and 20260717010600 (launch) rewritten to the teams-inbox design and the isolated-env
  gate; both stay in `todo/`, NOT authorized. `drive_authorized` **left unset**: the remaining set
  is not drive-ready on the shared host — its DRIVE is parked for an isolated/attended environment.
- 2026-07-19 — mission replanned — 20260717010500-claude-steering-rewire.md
- 2026-07-19 — mission replanned — 20260717010600-claude-session-create-launch.md
- 2026-07-19 — **`/monitor` pass: nothing left is unattended-drivable; consolidated the remaining
  live proofs into one owner-attended, isolated-environment ticket.** A monitor leaf re-checked the
  authorized set against the current tree and found the prior replan already landed (the pty/tmux
  retirement + teams-inbox ruling of commits `1d6ffcc`/`89fd431`/`8180594`) and the launch hermetic
  implementation already shipped (`a73fa01` / `v0.0.81`). The only remaining work — steering's
  message-schema capture + live fire, the launch `--bg` live fire, and the composed launch→steer
  proof — every step spawns or observes a REAL Claude Code process, which the mission's ABSOLUTE
  safety prohibition bars on this shared host and rules out of unattended / `/monitor` scope. Rather
  than drive anything unsafe or wire a *guessed* inbox schema (which would silently steer nothing —
  the exact bug this mission exists to kill), the leaf filed
  `20260719231005-claude-live-round-owner-attended.md`: a single owner-attended, isolated-box round
  covering items 5 and 6's live proofs, left in `todo/` and NOT drive-authorized. `drive_authorized`
  stays unset — deliberately, matching the author's standing decision that the remaining set is not
  drive-ready on the shared host. No code, version, or CLI surface changed this pass.
- 2026-07-22 — ticket added — 20260719105527-tmux-session-teardown-must-not-kill-server.md
- 2026-07-22 — ticket added (rider: compiled slack driver bugfix, owner-directed onto this overnight queue) — 20260721190756-slack-driver-channel-id-resolution.md
- 2026-07-22 — strategy created and linked — ai-sessions-are-ordinary-qfs-surfaces
- 2026-07-22 — mission replanned for the overnight run - owner ruling 2026-07-22: a container on this host (docker=podman) qualifies as the isolated box, so the remaining set is unattended-drivable in-container; Experience section added per the mission schema; drive_authorized stamped — mission.md
- 2026-07-22 — live round re-ruled autonomous-in-container (owner directive 2026-07-22); minimal claude credential copy into the container permitted, host claude state and sockets never mounted; tmux ticket gains container-only policies — 20260719231005-claude-live-round-owner-attended.md
- 2026-07-22 — ticket added (rider: slack user-token DM write channel_not_found bugfix; QG hermetic, no third-party DM) — 20260722171439-slack-driver-user-token-dm-write-channel-not-found.md
- 2026-07-22 — ticket archived — 20260721190756-slack-driver-channel-id-resolution.md
- 2026-07-22 — ticket archived — 20260722171439-slack-driver-user-token-dm-write-channel-not-found.md
- 2026-07-22 — ticket archived — 20260719105527-tmux-session-teardown-must-not-kill-server.md
