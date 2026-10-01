---
created_at: 2026-10-01T21:05:49+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20261001210535-the-host-spawns-a-claude-code-agent-on-a-cluster-member-by-query-and-reads-its-answer-back.md]
merge_policy:
verification_handoff: 
---

# The host spawns a Claude Code agent on a member and reads its answer

## Overview

The cluster can observe members and let a member borrow a host mount, but the host cannot
instruct a member. Add a one-shot agent spawn: the host sends a prompt to a chosen member
over the existing authenticated WebSocket; the member runs `claude -p` (non-interactive
Claude Code) in a working directory and streams back its final answer; the host records each
run so it can be listed and read. One prompt fanned out to several members, answers gathered,
is the demonstrated use (the developer's "each machine tells a joke" example).

Builds on the cluster crate from mission
`a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools` (PR #146);
drive it on that branch or after it merges.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:design` — default deny on the member, least privilege, audit every spawn

## Key Files

- `packages/qfs/crates/cluster/src/frame.rs` — `Spawn{id, prompt, cwd, allowed_tools}` host→member, `SpawnResult{id, ok, output, exit_code}` member→host
- `packages/qfs/crates/cluster/src/member.rs` — opt-in spawn runner (`qfs cluster join --accept-spawn`)
- `packages/qfs/crates/cluster/src/host.rs`, `registry.rs` — dispatch to a connected member, in-memory run table, `/api/cluster/agents`
- `packages/qfs/crates/cmd/src/lib.rs`, `packages/qfs/crates/qfs/src/cluster.rs` — `qfs cluster spawn` / `qfs cluster agents`
- `packages/qfs-viewer/packages/cluster-console/` — an Agents area in the console
- `docs/adr/0008-cluster-trust-model.md` — spawn section

## Implementation Steps

1. Member opt-in: `qfs cluster join --accept-spawn [--spawn-cwd DIR] [--spawn-timeout 600]`; without the flag a Spawn frame is refused. The member runs `claude -p <prompt>` with `--allowedTools` limited to what the host sent (default: none), in DIR (default: a fresh temp dir), killed at the timeout, output capped.
2. Host: `qfs cluster spawn --member <name>|--all "<prompt>" [--cwd DIR] [--allowed-tools ...] [--wait]` posts to a loopback-only `POST /api/cluster/agents` on the host listener, which dispatches over the member's live connection and records the run (id, member, prompt, status queued/running/done/failed, output, started, finished).
3. `qfs cluster agents [--id ID]` and `GET /api/cluster/agents` list runs; `--wait` blocks until all spawned runs finish and prints member → answer.
4. Audit each spawn and result on the host (`<config>/cluster/audit.log`).
5. Console: an Agents column (member, status, prompt, answer).
6. Hermetic tests with a fake `claude` executable on PATH: opt-out refuses, opt-in runs and returns output, timeout kills, --all fans out.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `qfs cluster spawn --all --wait "<prompt>"` returns one answer per opted-in member
- A member joined without `--accept-spawn` refuses and the run is recorded as failed
- Runs appear in `qfs cluster agents` and the console

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs-cluster` with the fake claude
- By hand on the live three-machine cluster: one joke prompt to all members, answers gathered

**Gate** — what must pass before approval:

- The workspace gates in `CLAUDE.md`

## Considerations

- A spawned agent runs as the member's OS user with that user's Claude Code login; the account pool is not used yet.
- No container sandbox in this slice (the vision's default podman is later work); the tool allowlist and opt-in are the guard.
- Spawning is a remote-code-execution surface by nature; that is why it is opt-in per member and loopback-only on the host.
