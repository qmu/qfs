---
created_at: 2026-08-05T11:33:00+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
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

## Final Report

Development completed as planned. Acceptance item 6's live half is proven, and the round found one
latent defect in the launcher — sent back as its own ticket, not patched here, exactly as the
Overview requires.

**Gate 3 first, because it is the one that could have been faked — the irreversible gate bites
live.** The same statement, committed without the acknowledgement:

```
$ qfs run --commit "insert into /hosts/local/claude/sessions values (…)"
{"preview":{… "irreversible":true …},"committed":false}
{"error":{"code":"irreversible_ack_required","kind":"commit_required",
  "message":"plan contains an irreversible effect (REMOVE / CALL);
             re-run with --commit-irreversible to apply (or in an interactive session)"}}
```

Nothing spawned. The gate was then opted into, never bypassed.

**Gate 1 — the recorded live round.** In an ephemeral Claude Code on the web execution container,
`QFS_CLAUDE_SESSIONS` and `HOME` both pointed at an isolated scratch tree:

```
$ qfs run --commit --commit-irreversible \
  "/hosts/local/claude/sessions |> limit 1
   |> extend prompt = 'Reply with the single word READY and stop.'
   |> set cwd = '…/live-work' |> set name = 'live-fire'
   |> select cwd, prompt, name
   |> insert into /hosts/local/claude/sessions"
{"preview":{"rows":[{"id":1,"verb":"INSERT","target":{"driver":"claude","path":"/claude/sessions"},
  "affected":{"exact":1},"irreversible":true}],"irreversible":[1],
  "total_affected":{"exact":1},"is_pure":false},"committed":true}     # T=16:05:48Z
```

A real process started: job `c0bbb006` under the scratch daemon, worker pid 19142, its own
`sessions/19142.json` record, and it ran the prompt (`last_message: "READY"`).

**Gate 2 — immediately addressable.** The **first** `/hosts/local/claude/sessions` query after the
commit (T=16:05:56Z, ~8 s later, no restart and no retry loop) returned the new session as a full
row:

```json
{"id":"c0bbb006-d691-4e72-bed5-2877ff27785b",
 "cwd":"…/live-work","name":"live-fire","status":"idle","last_message":"READY"}
```

No delay a caller must know about was needed; the row appeared on first ask. The bound is the
session process writing its own liveness record, which it does as it starts.

**Gate 4 — the host is provably untouched.** The container's own `~/.claude/sessions/` held exactly
`521.json` + its key, byte- and mtime-identical (15:38:06) before and after; no new `claude` process
exists on it.

**Gate 5 — cleaned up.** All three spawned jobs stopped, the scratch daemon supervisor killed, the
roster empty, and the only `claude` process remaining is this session itself. No orphan.

**The finding.** `claude --bg … --name <name>` in 2.1.233 prints a **three**-field banner —
`backgrounded · 4f89081e · spike-solo` — and `parse_backgrounded_id` returns the line's last token,
so a named launch captures the *name* where the id was meant. It is latent today (the applier
discards the launched id and returns an affected count), which is why this round still passed its
own gate. Sent back to the launcher as
`20260816161143-launcher-captures-the-name-not-the-session-id.md`; nothing was patched here.

### Discovered Insights

- **Insight**: The launch is unreachable through the literal `insert into … values (…)` form. That
  form binds positionally against the target relation's schema, and `prompt` is not a column of
  `/claude/sessions` — so the obvious spelling dies at commit with "session launch is missing the
  `prompt` column". Only the `|> extend / |> set / |> select cwd, prompt, name |> insert into`
  pipeline works.
  **Context**: The launch surface has shipped since v0.0.81 with hermetic tests that build the row
  batch directly, so nothing ever exercised the path a human or agent would actually type.

- **Insight**: A live round is worth running even when its gate is expected to pass. All five gates
  passed, and the round still surfaced a wrong-id defect and an unreachable statement form — neither
  visible to any hermetic test, because both live in the gap between the fake and the real CLI.
  **Context**: The two defects sit either side of the launcher: what the CLI prints back, and what
  the query language can express going in.
