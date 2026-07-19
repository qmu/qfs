---
created_at: 2026-07-17T01:06:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, UX]
effort:
commit_hash:
category: Added
depends_on: [20260717010400-claude-path-canon-hosts-move.md, 20260717010500-claude-steering-rewire.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Launching a session: design brief first, then CREATE/spawn ships

## Overview

Mission acceptance item 6 (owner capability 2: "launch another session"). Greenfield, verified:
no spawn, no CREATE grammar for sessions, no `Command::new` anywhere in the claude lane;
`Capabilities` are Select-only on the sessions relation; t64 pre-ruled only teardown
(irreversible `Remove`). There is no design to implement yet — **the design brief is the first
deliverable**, reviewed by the owner before code:

1. **What a launch is**: `claude -p` one-shot vs a persistent interactive/daemon session; which
   the mission's "launch another session" means (the steering capability implies a session that
   outlives the launch and can be steered).
2. **Grammar**: plausibly `CREATE SESSION ON /hosts/<host>/claude AT '<cwd>' [PROMPT '...']` or
   an INSERT into the sessions relation — pick the shape that keeps "everything is a path" true
   and DESCRIBE/preview legible; the sessions relation is Select-only today, so whatever verb
   lands must widen capabilities deliberately.
3. **Gating**: launching spends money and starts an autonomous actor. Decide reversibility
   class (a launch is not reversible in the qfs sense — the turn runs) and whether it takes the
   irreversible acknowledgement gate like `Remove`/`mail.send`.
4. **Identity**: the spawned process runs as the operator (same uid, same `~/.claude`) — state
   this and its consequences (billing account, permissions) explicitly; the agents-as-principals
   mission is a different axis (kept separate by owner decision, 2026-07-16) and must not be
   accidentally merged here.
5. **Addressability**: how the new session's id returns to the caller (the launch's RETURNING
   row) and how quickly it appears in `/hosts/<host>/claude/sessions` (the store writes
   `sessions/<pid>.json` on boot — verify timing).

Then ship the ruled design behind the existing seams: a launch effect in the applier lane
(never in the pure driver crate), preview showing exactly what would spawn, fail-closed without
a configured store.

## Policies

- `workaholic:planning` — the brief precedes implementation; owner rules the grammar and gating
  before code lands.
- `workaholic:design` / access control — launch runs as the operator on the local host only;
  the remote hop stays out of scope (tunnel seam).
- `workaholic:implementation` / preview-commit — a launch previews legibly and only COMMIT
  spawns.

## Quality Gate

1. The design brief is recorded (blueprint or mission-adjacent doc) and owner-acknowledged
   before the implementing commits.
2. Live proof: a qfs query launches a session; the new id then appears in the sessions relation
   and is steerable (capabilities 2+4 compose).
3. Hermetic tests: the spawn effect behind a fake launcher seam; preview/commit gating; failure
   modes (bad cwd, store unconfigured) structured and secret-free.

## Considerations

- Do not let the launch surface become a shell-injection vector: cwd and prompt are data, the
  binary path is configuration, nothing user-supplied is interpolated into a shell line.
- A launched session that immediately exits (bad prompt) must still be visible post-mortem —
  decide what the sessions relation shows for it (the liveness filter hides dead pids).

## Status (2026-07-17 — brief delivered; implementation awaits the owner)

The first deliverable is done: the design brief is recorded at
`.workaholic/missions/active/claude-code-sessions-are-queryable-and-steerable-as-qfs-paths/design-brief-session-launch.md`
(recommends: launch = `claude --bg` persistent background session; grammar = INSERT into the
sessions relation per blueprint §3, no `CREATE SESSION` noun; irreversible-gated; runs as the
operator; id via `RETURNING`; three open questions listed). Quality gate 1 requires owner
acknowledgment BEFORE any implementing commit, so this ticket stays in todo until the owner
rules. Also blocked serially: QG 2's live proof composes launch with steering (capability 4),
and the steering ticket (20260717010500) is itself pending a medium ruling.

## Status update (2026-07-18 — brief acknowledged; QG1 satisfied)

The owner ruled all three open questions in a `/monitor` replan (AskUserQuestion, 2026-07-18) —
recorded in the brief's "Owner rulings" section: **INSERT grammar** (`Sessions` widens to
`Select+Insert`, `RETURNING id`), **irreversible-gated** launch (`--commit-irreversible` / ack),
and an accepted optional **`name` column**. **Quality gate 1 (owner acknowledgment before
implementing commits) is now satisfied.**

Drive-readiness of the two remaining gates:

- **QG3 (hermetic) — drive-ready now.** The launcher effect behind a fake seam, INSERT grammar +
  capability widening, the irreversible preview/commit gate, the `name` column, and the
  bad-cwd / store-unconfigured failure modes can all be implemented and tested hermetically
  without an owner or any spend.
- **QG2 (live proof) — owner-attended, still blocked.** It launches a real session and composes
  with steering (capability 4), whose transport medium remains undecided pending the
  owner-attended probe in ticket `20260717010500`. The live, money-spending composition waits for
  an owner-attended session; the hermetic implementation does not depend on it.

Dependency note: the `depends_on: 20260717010500` edge exists because QG2 composes launch with
steering. The **hermetic implementation** (QG1+QG3) does not need steering wired — only the
live-proof gate does. A driving session may land QG1+QG3 and leave QG2 for an owner-attended run.

## Status update (2026-07-19 — hermetic launch DONE; only the owner-attended live fire remains)

**QG3 (hermetic) is complete** (commit `a73fa01`, qfs `v0.0.81`). Shipped:

- **Grammar + capability.** `INSERT INTO /hosts/<host>/claude/sessions` is a session launch; the
  `Sessions` relation widened `Select` → `Select+Insert` deliberately. The qfs surface names the
  columns after `VALUES`: `insert into /hosts/local/claude/sessions values (cwd, prompt, name)
  ('…','…','…') returning id`. No `CREATE SESSION` noun. `RETURNING id` types the projection
  against the sessions schema.
- **Irreversible gate.** `ClaudeDriver::write_irreversible` flags the sessions `INSERT`
  irreversible (the `Remove`/`mail.send` precedent). Verified end-to-end on the built binary:
  PREVIEW is effect-free and marks it irreversible; `--commit` without `--commit-irreversible`
  fails closed (`irreversible_ack_required`); `--commit --commit-irreversible` spawns.
- **Launcher seam in the applier lane.** A new `SessionLauncher` trait + `LaunchSpec` (pure driver
  crate, I/O-free); the real `ClaudeCliLauncher` (binary crate) runs `Command::new(<configured
  binary — `QFS_CLAUDE_BINARY`, default `claude`)` with cwd/prompt/name as **discrete arguments**,
  never a shell line. Optional `name` accepted. Fail-closed: no store ⇒ no applier; no launcher ⇒
  the `INSERT` is refused.
- **Hermetic tests** behind the fake launcher seam: spawn/route, name-optional, no-launcher
  fail-closed, malformed payload; plus real-launcher tests (a shell-metacharacter prompt lands
  verbatim as one argv entry; bad cwd → structured secret-free `LaunchFailed`; store-unconfigured).

**QG2 (live proof) is NOT done and the launch acceptance item is deliberately NOT ticked.** The
real, money-spending launch composed with steering (capability 4) is the owner-attended proof
(mission acceptance ~164); it composes with the steering medium ruled in `20260717010500` and must
run in the owner's attended session. This ticket therefore **stays in `todo`** — only the
owner-attended live fire remains.

## Replan (2026-07-19 — the live fire spawns real processes, so it runs ONLY in an isolated environment)

Owner ruling, 2026-07-19: launching a session is inherently process-spawning — QG2 runs
`claude --bg`, which starts a real background process. On this shared host (which runs the owner's
live Claude Code sessions) exercising the spawn/steer legs repeatedly crashed the parent session,
so **QG2's live fire is gated to an isolated environment** (a container/VM with no live sessions)
and is **out of unattended / `/monitor` scope**. The composed launch→steer proof rides on the
steering medium, now settled as the **teams inbox** (a durable enqueue that kills no process — see
`20260717010500`), so the *transport* is no longer the blocker; the process-spawn is.

Standing after this replan:

- **QG1 (owner acknowledgment) — satisfied** (2026-07-18 rulings, above).
- **QG3 (hermetic implementation) — DONE** (commit `a73fa01` / `v0.0.81`, above): INSERT grammar,
  `Sessions` widened to Select+Insert, irreversible gate, launcher seam behind a fake, hermetic
  tests. No further shared-host work exists on this ticket.
- **QG2 (live proof) — parked for an isolated/attended environment.** It spawns a real session and
  composes with steering's live fire; both must run in an isolated box, never the shared host. The
  ticket stays in `todo/`, NOT drive-authorized.
