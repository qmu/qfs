---
created_at: 2026-08-05T11:33:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort:
commit_hash:
category: Added
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
merge_policy: review
---

# Launch live fire: an INSERT spawns a real session that is immediately addressable

## Overview

Acceptance item 6's design and implementation are **already shipped** — commit `a73fa01`, v0.0.81:
the INSERT grammar, `Sessions` widened to Select+Insert, the irreversible gate, the launcher seam
behind a fake, and hermetic tests. What the item still names as unmet is its live half: a real
`claude --bg` process spawned by a real query, which the mission's environment constraint confines
to a container.

This ticket is only that live half. It carries no design work and no implementation — if it fails,
the finding goes back to the launcher, it is not patched here.

It has **no `depends_on`**: the launch path does not go through the teams inbox, so it is
independent of the steering chain and can run in parallel with it.

## Environment — non-negotiable

Runs **inside a container** with no live sessions (mission Scope environment constraint; owner
container ruling 2026-07-22). A launch spawns a real OS process; doing that on the shared host is
what the constraint exists to prevent. Never mount the host's `~/.claude`, never inherit the host's
tmux sockets or `TMUX_TMPDIR`.

## Scope

**In scope.**

1. In a container, run a real `qfs` INSERT that launches a session, through the irreversible gate
   (so the gate is exercised, not bypassed).
2. Confirm the process actually started — the spawned `claude` process exists, and the launch
   returned the new session's id rather than a placeholder.
3. **Confirm the new id is addressable immediately**: querying `/hosts/<host>/claude/sessions`
   returns a row for it, carrying the schema's columns, without a restart or a delay the caller has
   to know about. If a delay is unavoidable, measure it and record it as a stated property rather
   than leaving callers to discover it.
4. Confirm the irreversible gate refuses the same INSERT when the caller has not opted in — the
   gate is proven to bite live, not only in a hermetic test.
5. Tear the spawned session down inside the container when the run finishes.

**Out of scope.** Steering the launched session (that composition is a later step, and it depends on
the steering chain landing first). Any change to the launcher.

## Policies

- workaholic:development / hermetic gates — the deliberate, container-bounded exception the mission
  Scope carves out. The hermetic half of this capability already shipped; only what cannot be
  hermetic is here.
- workaholic:design / irreversibility — a launch is gated as irreversible, and this proves the gate
  behaves live exactly as the hermetic test claims.

## Quality Gate

1. A recorded live round: the real INSERT, its raw output, the spawned process observed, and the
   container it ran in.
2. The launched session's id appears in a `/hosts/<host>/claude/sessions` query in the same run,
   with a non-empty row. If any delay is needed before it appears, the measured delay is recorded.
3. The irreversible gate is shown refusing the un-opted-in form of the same statement.
4. The host is provably untouched: no new `claude` process on the host, and the host's session
   liveness registry (`~/.claude/sessions/`) is unchanged across the run.
5. The spawned session is cleaned up; the container leaves no orphan process behind.

## Considerations

- The composed proof the mission's Scope mentions — launch a session, then steer it — is deliberately
  not here. It needs both chains landed, and folding it in would make this ticket's acceptance depend
  on work it does not do.
- If a container with working Claude Code credentials cannot be provisioned, this is an external
  blocker: record it with the exact failure and stop. Never fall back to the shared host.
