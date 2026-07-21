---
created_at: 2026-07-19T10:55:27+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# tmux-backed session teardown must use `kill-session` on an isolated socket, never `kill-server`

## Overview

The mission `claude-code-sessions-are-queryable-and-steerable-as-qfs-paths` evaluates tmux as a
medium for launching and steering Claude Code sessions (`new-session` to start one, `send-keys`
to steer it). While verifying that this path passes argv safely — no shell injection through
`send-keys` / `new-session` — the verification harness tore its throwaway tmux server down with:

```
tmux -f /dev/null kill-server
```

`kill-server` destroys **every session on that tmux server**, not only the one the harness
created. It is safe just so long as the server is guaranteed to be a private, throwaway one, and
that guarantee currently rests entirely on an environment variable (`TMUX_TMPDIR`) being exported
into the same shell. A single missed export, a variable not inherited by a child process or
subshell, or a copy of the teardown line without its setup, and the `kill-server` lands on the
developer's **default** tmux socket and takes down all of their real, unrelated sessions.

This is a foot-gun in the place it hurts most: a developer running the product's own session
tooling loses the very terminal sessions they are working in.

## The rule this asks for

Teardown of any tmux server or session the product (or its tests) creates must be safe by
construction, not by remembering to isolate:

1. **Always run on a dedicated socket.** Give every product-spawned tmux server an explicit,
   unique socket (`tmux -L <unique-name> …`) rather than depending on `TMUX_TMPDIR`. A `-L` name
   travels on every subsequent command as an argument, so it cannot be silently dropped the way an
   un-exported env var can.
2. **Tear down by target, never wholesale.** Use `kill-session -t <session-name>` (or a
   `kill-server` scoped to the dedicated `-L` socket). Never issue a bare `kill-server` that can
   resolve to the default socket.
3. **Assert isolation before killing.** Before any teardown, verify the socket being torn down is
   the dedicated one (e.g. the `-L` name is set); no-op otherwise.

## Where it lives

- The tmux launch / steer path under the claude session driver
  (`packages/qfs/crates/driver-claude/`) and any verification or e2e harness that spawns tmux to
  exercise it.
- Wherever a teardown line issues `kill-server`, replace it per the rule above and add a guard so
  the default socket can never be the target.

## Acceptance

- No code path or test issues a `tmux kill-server` (or `kill-session`) that can resolve to the
  developer's default tmux socket.
- Every product-spawned tmux server is created with an explicit `-L <unique>` socket and torn down
  by a targeted `kill-session`, guarded by an isolation check.
- A test demonstrates that running the teardown with the isolation misconfigured (no dedicated
  `-L` socket) refuses rather than killing the default server.
