---
created_at: 2026-08-05T11:32:00+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
type: enhancement
layer: [Infrastructure]
effort:
commit_hash:
category: Added
depends_on: [20260805113100-append-instruction-writes-the-lead-teams-inbox.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
merge_policy: review
---

# Steering live fire: an INSERT is observed by a real session, in a container

## Overview

This is the proof that closes acceptance item 5. Everything before it is hermetic: fixture
directories and captured schemas. This ticket runs the real thing — a qfs INSERT into
`/hosts/<host>/claude/sessions/<id>/instructions` against a **live Claude Code session**, and
observes that session receive it.

It is a separate ticket from the implementation for one reason: it is the only step that touches a
real process, and the mission's environment constraint confines that to a container. Splitting it
also keeps the acceptance honest — item 5 ticks here, when steering is *observed to work*, not when
the code that should make it work is merged.

## Environment — non-negotiable

Runs **inside a container** with no live sessions (mission Scope environment constraint; owner
container ruling 2026-07-22). Never mount the host's `~/.claude`, never inherit the host's tmux
sockets or `TMUX_TMPDIR`. The retired pty/tmux transport crashed the owner's parent session on this
shared host more than once; the teams inbox is non-process-killing by construction, but the *target
session* in this proof is a real process and must not be one of the owner's.

## Scope

**In scope.**

1. In a container, start a Claude Code session, form a team if the spike found that is required for
   an inbox to exist, and read its id through `/hosts/<host>/claude/sessions` — the same query the
   read path already serves.
2. Run a real `qfs` INSERT into that session's `.../instructions` and commit it.
3. **Observe the target session receive it.** Not "the file changed" — the session acting on the
   message, or at minimum the message disappearing from the inbox by the session's own drain rather
   than by anything the test did. State which of the two was observed and why it is sufficient.
4. Record the transcript: the query, the raw output, the observation, and the container it ran in.

**Out of scope.** The launch live fire (its own ticket). Any code change — if the proof fails, that
is a finding that sends work back to the implementation ticket, not a licence to patch here.

## Policies

- workaholic:development / hermetic gates — this is the deliberate exception the mission's Scope
  carves out, and it is bounded by the container. Everything that can be hermetic already is.
- workaholic:implementation / observability — the evidence recorded here is what makes the
  acceptance item's tick true. A tick backed by "it should work" is the failure this ticket exists
  to prevent.

## Quality Gate

1. A recorded live round: a real INSERT, against a real session in a container, observed by that
   session. The raw command and its output are in the record.
2. The observation distinguishes **the session drained it** from **the file changed**. If only the
   latter can be shown, say so plainly and do not tick the acceptance item.
3. The host's sessions are provably untouched: the host's `~/.claude/teams/*/inboxes/*.json` are
   unchanged (content and mtime) across the run.
4. A session with no inbox, exercised live, produces the same structured refusal the hermetic test
   asserts — the fail-closed path is proven on a real session, not only on a fixture.

## Considerations

- If the spike answered "an unsolicited inbox is never drained", then a solo session cannot be
  steered and this proof must use a team-formed session. Record that limitation in the run notes so
  the acceptance item's wording and the mission's Experience section can be corrected to match what
  actually ships.
- If a container with working Claude Code credentials cannot be provisioned, this is an external
  blocker: record it with the exact failure and stop. It must never be run on the shared host.

## Final Report

Development completed as planned. Acceptance item 5's live half is proven, in the strong form the
Quality Gate asks for.

**Gate 1 — a recorded live round.** In an ephemeral Claude Code on the web execution container
(`container_017nHuEVymH8tUQ4TAXAPoXL--claude_code_remote--49d3e1`), with `QFS_CLAUDE_SESSIONS`
pointed at an isolated scratch `HOME`:

```
$ qfs run --commit "/hosts/local/claude/sessions |> where id == 'c0bbb006-d691-4e72-bed5-2877ff27785b'
    |> extend instruction = 'Use the Write tool to create the file …/qfs-steered.txt containing
       exactly the word QFSSTEERED. Then stop.'
    |> select instruction
    |> insert into /hosts/local/claude/sessions/c0bbb006-…/instructions"
{"preview":{"rows":[{"id":1,"verb":"INSERT","target":{"driver":"claude",
  "path":"/claude/sessions/c0bbb006-…/instructions"},"affected":{"exact":1},
  "irreversible":false}],"irreversible":[],"total_affected":{"exact":1},"is_pure":false},
 "committed":true}
```

The target session was itself launched by a real qfs INSERT minutes earlier (ticket
`20260805113300`), and its id was read back through `/hosts/local/claude/sessions` — the same query
the read path serves — before being addressed. Steering is correctly **reversible**: no
`--commit-irreversible` was needed, matching `write_irreversible`.

**Gate 2 — the session acted; this is not "a file changed".** Commit at 16:06:18Z, and the session's
own job timeline:

```
{"at":"16:06:18.551Z","state":"done",   "detail":"Reply with the single word READY and stop.","text":""}
{"at":"16:06:22.229Z","state":"working","detail":"Writing qfs-steered.txt","text":""}
{"at":"16:06:24.198Z","state":"blocked","detail":"file created; awaiting READY confirmation",
 "text":"Done — created the file with the requested content."}
```

`qfs-steered.txt` contains `QFSSTEERED`. The session transitioned `done → working → blocked` on its
own, narrated the work, and produced the artifact ~4 s after the commit. That is the session acting,
not a file mutating.

**Gate 3 — the host is provably untouched.** No `~/.claude/teams/*/inboxes/*.json` exists anywhere
outside the scratch `HOME` (the container has no `teams/` directory at all), and the container's own
`~/.claude/sessions/` was byte- and mtime-identical across the whole run (15:38:06, before any of
this began). Every spawned process was torn down; the only `claude` process left is this session.

**Gate 4 — the fail-closed path proven live, not only on a fixture.** Both refusals came back from
real commits against the real store, matching the hermetic assertions word for word:

```
unknown session:  commit_failed … "claude session `no-such-session-id` not found"
leftover record:  commit_failed … "claude session `s-ghost` is a leftover record:
                                   its process is no longer running, so there is nothing to steer"
```

**The Considerations' limitation did not materialise.** The spike answered "an unsolicited inbox is
never drained", which would have confined steering to team-formed sessions — but the medium that
replaced it steers any live session, so acceptance item 5's wording and the mission's Experience
section need no correction.

### Discovered Insights

- **Insight**: The `|> extend … |> select … |> insert into` pipeline is the only way to express a
  named-column write from the CLI. The literal `insert into … values (…)` form binds positionally
  against the target relation's schema, so a launch written that way dies at commit with "missing
  the `prompt` column" — `prompt` is not a column of `/claude/sessions` at all.
  **Context**: Both `/claude` writes name columns the read schema does not declare. Any doc or skill
  teaching these writes has to teach the pipeline form, or teach a statement the binary rejects.

- **Insight**: The whole live round needed no container-in-container. The mission's environment
  constraint exists to keep a live-fire away from the developer's own sessions on a shared host; an
  ephemeral cloud container satisfies that by construction — it holds no developer session, and it
  is discarded when the session ends.
  **Context**: Two tickets carried "runs inside a container, never on the shared host" as
  non-negotiable and were blocked on provisioning one. The environment the routine already runs in
  is a strictly stronger isolation than the constraint asks for.
