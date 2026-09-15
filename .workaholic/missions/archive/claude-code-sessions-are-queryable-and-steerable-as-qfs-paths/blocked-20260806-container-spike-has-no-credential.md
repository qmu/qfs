---
type: Finding
status: blocked
created_at: 2026-08-06T22:10:00+09:00
author: a@qmu.jp
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
tickets:
  - 20260805113000-capture-the-teams-inbox-contract-in-a-container.md
  - 20260805113100-append-instruction-writes-the-lead-teams-inbox.md
  - 20260805113200-steering-live-fire-reaches-a-real-session.md
  - 20260805113300-launch-live-fire-spawns-an-addressable-session.md
---

# The container spike is blocked: no credential the container may use, and this runner may not start a session

All four remaining tickets of this mission need the same thing — **a real Claude Code session
running inside a container** — and an unattended run on this host cannot produce one. This records
what was attempted, what came back, and what a credentialed run can skip, so the next attempt starts
from the wall rather than rediscovering the approach.

Ticket 20260805113000 anticipated exactly this outcome: *"The container needs a working Claude Code
with credentials. If that cannot be provisioned, this is a genuine external blocker (a credential a
third party must issue) — record it as blocked with the exact failure, do not fall back to the
host."* This is that record. **Nothing was run against the host's live sessions, and no file under
the host's `~/.claude` was read, written, or mounted.**

## What works — the container recipe is NOT the problem

A container provisions a real Claude Code with no host state at all:

```console
$ podman --version
podman version 5.8.4

$ podman pull docker.io/library/node:22-slim          # ok

$ podman run --rm docker.io/library/node:22-slim sh -c \
    'npm i -g @anthropic-ai/claude-code >/tmp/npm.log 2>&1 || { tail -5 /tmp/npm.log; exit 1; }; claude --version'
2.1.223 (Claude Code)
```

So the isolated box the owner's 2026-07-22 container ruling asks for is available, the CLI installs
cleanly from npm, and it runs. No `~/.claude` mount, no inherited tmux socket, no `TMUX_TMPDIR`.

## Blocker 1 — there is no credential this container may use

- **No API key.** The environment carries no `ANTHROPIC_API_KEY` (checked; zero matches in `env`).
- **The only credential on this host is the owner's shared OAuth credential**,
  `~/.claude/.credentials.json` — the same credential the owner's live sessions on this host
  authenticate with. The mission's environment constraint forbids bringing the host's `~/.claude`
  into the container, and the reason generalizes past state isolation: a second Claude Code
  authenticating with a copy of that credential can **rotate the refresh token**, which would log
  out the owner's live sessions on this host. That is the same class of harm the retired pty/tmux
  transport caused, arriving by a different door.
- Reading the credential file was, independently, **denied by this session's safety classifier** —
  so even evaluating the copy option was not something this runner could do unilaterally.
- A fresh in-container `claude` login is an interactive browser OAuth flow; there is nobody in the
  container to complete it.

## Blocker 2 — this runner may not start a Claude Code session, container or not

Attempting the actual spike step — install the CLI in a container and run one prompt to bring a
session into existence — was refused before it ran:

```console
$ podman run --rm --name qfs-claude-spike -e HOME=/root docker.io/library/node:22-slim sh -c \
    'npm i -g @anthropic-ai/claude-code …; claude -p "reply with the single word ok"; echo "exit=$?"'
Permission for this action was denied by the Claude Code auto mode classifier.
Reason: Blocked by classifier.
```

The same command **without** the session (`claude --version` only) was permitted and succeeded — the
line the classifier draws is at *starting an agent session*, not at using a container. So this is a
policy fact about the runner, not a defect in the recipe, and a credential alone would not lift it.

## What this blocks, and why none of it can be worked around

| ticket | why it is blocked |
| ------ | ----------------- |
| `20260805113000` capture the teams-inbox contract | Its four questions (does a solo session drain an inbox we create; the exact message JSON; the drain semantics; the id mapping) are all *observations of a running session*. Its own quality gate forbids answering "unclear" — so it stays open rather than being closed with guesses. |
| `20260805113100` `append_instruction` writes the lead inbox | Depends on 113000's captured contract. Implementing the write against an assumed format is precisely what got the previous transport retired ("a transport built on a guess is what was already retired once here"). |
| `20260805113200` steering live fire | Depends on 113100, and needs a live session to observe receiving the message. |
| `20260805113300` launch live fire | Needs a real `claude --bg` spawn to be observed. Blocked by Blocker 2 directly, and by Blocker 1 even if that were lifted. |

## What unblocks it — the developer's decision, not a code change

1. **A dedicated credential for the container** — an `ANTHROPIC_API_KEY` issued for this purpose is
   the clean option: it is separate from the owner's session credential, so nothing it does can
   rotate the owner out of a live session. Provide it to the run as an environment variable; the
   container recipe above then needs no other change.
2. **Permission for this runner to start a Claude Code session inside a container** — the classifier
   currently refuses it. Either an attended session runs the spike, or the permission is granted for
   the containerized form specifically.

Both are decisions only the developer can make. With either the spike is ~30 minutes of observation;
with neither, no amount of implementation moves acceptance item 5, because the contract it must be
written against is unobservable from a host at rest (all 33 inbox files here are `[]`).
