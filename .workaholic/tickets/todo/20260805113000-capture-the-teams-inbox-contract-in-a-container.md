---
created_at: 2026-08-05T11:30:00+09:00
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

# Capture the teams-inbox contract in a container, before anything is written against it

## Overview

Acceptance item 5 says steering must reach a real session by appending to that session's teams
inbox. Two facts the implementation depends on are **not knowable from this host at rest**, and both
were checked rather than assumed (2026-08-05, read-only, no process touched):

1. **A solo session has no inbox.** Every session gets
   `~/.claude/teams/session-<short-id>/config.json`, but the `inboxes/` directory only exists for a
   session that actually formed a team. Measured: 37 team directories, **7** with `inboxes/`; of 6
   live sessions sampled, **1** had one. Whether creating `inboxes/<member>.json` ourselves for a
   solo session gets drained, or whether the directory's existence is a side effect of team
   formation and an unsolicited file is simply never read, **cannot be determined without a running
   session to observe**.
2. **The message shape is unobservable at rest.** All 33 inbox files on this host are `[]`. A
   running session drains its inbox, so the on-disk steady state carries no example. The wire format
   — field names, whether a message is an object or a string, what marks sender and recipient — has
   to be captured while a session is receiving.

This ticket answers both, in a container, and produces the facts the next ticket implements against.
It exists as its own ticket because writing the transport first would mean **guessing the format and
the scope**, and the previous transport for this same capability (qfs-owned pty/tmux) was retired
precisely for being built against an assumption about someone else's internals.

## Environment — this is the whole reason the ticket is separate

Runs **inside a container**, never on the shared host (mission Scope environment constraint; owner
container ruling 2026-07-22 — the host's `docker` is podman 5.8.4). Never mount the host's
`~/.claude`, never inherit the host's tmux sockets or `TMUX_TMPDIR`. The host runs the owner's live
sessions and driving this against them has crashed sessions before.

## Scope

**In scope.** Stand up a container with a real Claude Code able to form a team, and record:

1. **Solo-session behaviour.** With a session that has NOT formed a team, create
   `~/.claude/teams/session-<short>/inboxes/<lead-member>.json` containing one well-formed message
   and observe whether the session drains it. Record the answer either way, with what was observed.
2. **The message schema.** With a session that HAS formed a team, capture at least one message
   in flight — poll the inbox file while a message is being delivered, or read it before the drain.
   Record the exact JSON: every field, its type, and which fields are required.
3. **The drain semantics.** Whether the file is emptied to `[]` or deleted; whether an append while a
   drain is in progress is lost; whether the array is ordered.
4. **The id mapping.** Confirm that `config.json`'s `leadSessionId` is the session UUID the
   `/hosts/<host>/claude/sessions` rows carry, and that the team directory is
   `session-<first-8-of-uuid>` — the implementation resolves a session id to an inbox through this.

**Out of scope.** Any qfs code change. This ticket produces recorded facts, not an implementation.

## Policies

- workaholic:implementation / objective-documentation — the output is a factual record of observed
  behaviour, written so the next ticket can be implemented from it without re-running the container.
  "It appeared to work" is not a finding; the observed bytes are.
- workaholic:design / 「推測するな、宣言して拒否せよ」 — the reason this precedes implementation is
  that the alternative is guessing an external format. A transport built on a guess is what was
  already retired once here.

## Quality Gate

1. A written record lands in the mission directory (a `design-brief-*.md` beside `mission.md`)
   answering all four questions above, each with the observation that supports it — the actual JSON
   captured, not a paraphrase.
2. The solo-session question is answered **yes or no**, explicitly. "Unclear" is a failure of this
   ticket, not an acceptable outcome: if the container could not produce a solo session that drains,
   say what was attempted and what came back, and record the ticket blocked with that raw output.
3. Every command that touched a process is recorded with the container it ran in, so the run is
   reproducible and provably not on the host.
4. No file under the host's `~/.claude` is modified. Verified by checking the host's inbox files are
   still `[]` and their mtimes unchanged after the run.

## Considerations

- If the answer to (1) is **no** — an unsolicited inbox is never read — then acceptance item 5 can
  only promise steering for team-formed sessions, and its wording needs the developer's ruling
  before the implementation ticket is driven. Surface that immediately rather than implementing a
  narrower feature under the existing wording.
- The container needs a working Claude Code with credentials. If that cannot be provisioned, this is
  a genuine external blocker (a credential a third party must issue) — record it as blocked with the
  exact failure, do not fall back to the host.
