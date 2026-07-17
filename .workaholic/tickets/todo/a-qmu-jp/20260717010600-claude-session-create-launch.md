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
