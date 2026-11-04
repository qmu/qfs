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

## Final Report

### What was built

- **Frames** (`cluster/src/frame.rs`): `Spawn{id, prompt, cwd?, allowed_tools}` host→member, `SpawnResult{id, ok, output, exit_code?}` member→host.
- **Member runner** (`cluster/src/spawn.rs`, wired in `member.rs`): opt-in only (`MemberConfig.spawn: Option<SpawnConfig>`; `None` answers `spawn not accepted by this member`). Runs `claude -p <prompt>` via `tokio::process`, adding `--allowedTools <comma list>` only for a non-empty host list. The binary comes from `QFS_CLUSTER_CLAUDE_BIN`, else `claude` on PATH, else `~/.local/bin/claude`. Each run gets a fresh temp dir (removed afterwards) unless `--spawn-cwd` is set. A host `--cwd` is refused when there is no spawn root, and must canonicalize under the root when there is one. Stdin is null. The agent is killed at `--spawn-timeout` (default 600 s). stdout and stderr are each capped at 64 KiB. At most 4 runs at once. Runs are tokio tasks, so heartbeats continue.
- **Host run table** (`cluster/src/agents.rs`, held by `Registry::agents()`): per-member live links, runs `running → done|failed` (newest 500 kept). A run fails when its member is not connected, refuses, exits non-zero, times out, or disconnects. A result is accepted only from the member the run was sent to. Every spawn and result is audited to `<config>/cluster/audit.log` (`kind: spawn` / `spawn_result`).
- **Host API** (`host.rs`): `GET /api/cluster/agents[?id=N]` and `POST /api/cluster/agents`, both loopback-only. The POST additionally requires `Content-Type: application/json` (CSRF guard), a loopback `Host` (DNS-rebinding guard), and no foreign `Origin`. Body capped at 64 KiB, prompt at 32 KiB. `post_json` client helper.
- **CLI**: `qfs cluster join … --accept-spawn [--spawn-cwd D] [--spawn-timeout 600]`, `qfs cluster spawn (--member N | --all) "<prompt>" [--cwd D] [--allowed-tools T]… [--wait] [--host 127.0.0.1:7466]` (with `--wait`, prints `member → [run id] answer` and exits 1 if any run failed), `qfs cluster agents [--id ID] [--host …]`.
- **Console**: an Agents area (runs newest-first → detail with prompt, answer, tools, exit code, times). Tone colours for running/done/failed. Embedded bundle rebuilt.
- **Docs**: ADR 0008 has a new "Spawning an agent" section. The new `Command::new` site is added to the exec-inventory allowlist (`cmd/tests/exec_inventory.rs`) and to the blueprint's Command-execution assurance table, which that lock requires in the same PR.

### Gates

- `cargo build --workspace`: ok
- `cargo test --workspace --no-fail-fast`: 2887 passed, 0 failed
- `env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1`: 542 passed
- clippy `-D warnings`: clean
- `fmt --check`: clean
- `gen-docs --check` / `gen-skills --check`: in sync
- cluster-console: tsc, plgg-test 100% statements, 93% branches; `build-cluster-console.sh --check` up to date

Hermetic tests (`cluster/tests/spawn.rs`, fake `claude` script) cover: opt-out refuses, opt-in answers, `--all` fans out, the tool allowlist is passed only when given, heartbeats continue, the timeout kills, and the loopback POST/GET API works (a bad request is a 400 and creates no run). Unit tests cover the write guard, the cwd confinement, the argv shape, and result spoofing / unlink.

### Smoke (free port 17466, temp config home, fake claude)

One member joined with `--accept-spawn`, one without. Then `spawn --all --wait "tell a joke"`:
- `no → [run 1 (failed)] spawn not accepted by this member`
- `yes → [run 2] fake claude in /…/T/qfs-spawn-…: tell a joke`
- exit 1, because one run failed

`qfs cluster agents` listed both runs. `audit.log` had 2 spawn lines and 2 spawn_result lines. A `text/plain` POST and a foreign-`Origin` POST were both refused with 403. All smoke processes were killed afterwards.

### Deviations / notes

- A spawn to an unconnected member records a failed run instead of returning an HTTP error, so it shows up the same way as a refusal.
- The timeout kills only the `claude` process, not its process group. A grandchild it leaves behind can outlive it. Its pipes are abandoned after a 2 s grace, which was found by the timeout test under load. This is stated in the ADR.
- `spawn --wait` exits non-zero when any run failed, but still prints every answer.
- The live three-machine joke run is still to be done by hand. Not done here.
