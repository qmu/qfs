---
created_at: 2026-08-05T11:32:00+09:00
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
