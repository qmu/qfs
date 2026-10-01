---
created_at: 2026-09-28T02:11:22+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [20260928021122-cluster-members-join-a-host-over-an-authenticated-websocket-and-report-their-resources.md]
mission: a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools
merge_policy:
verification_handoff: 
---

# A member runs a statement against a host mount without receiving the credential

## Overview

A member cannot use a mount registered only on the host (the miss that started this: a Slack
account mount lives on another machine). This ticket lets a member send a statement over its
cluster link to be **executed on the host**, against the host's mounts, so the credential never
leaves the host. The host executes only mounts it has explicitly **granted** to that member.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:design` — least privilege, credentials stay with their owner, audit every borrowed call

## Key Files

- `packages/qfs/crates/cluster/` — `request`/`response` frames, grant table
- `packages/qfs/crates/qfs/src/cluster.rs` — host-side execution through the normal engine, member-side client
- `packages/qfs/crates/cmd/` — `qfs cluster grant` and `qfs cluster run`

## Implementation Steps

1. Host-side grant table: `qfs cluster grant <member> <mount>` (default: nothing granted).
2. `qfs cluster run [--commit] "<stmt>"` on a member sends the statement to its host via the running join daemon (local control socket) or a direct one-shot connection with the same token.
3. The host parses the statement, refuses it unless every path it touches is under a mount granted to that member, runs preview or commit through the same engine the CLI uses, and returns the result rows. The credential never goes into a frame.
4. Every borrowed execution is written to the host's audit log with member, statement and outcome.
5. Hermetic test with a fake mount: granted path works, ungranted path refused, no secret bytes in any frame.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A member runs an INSERT into a granted host mount and the host commits it
- A statement touching an ungranted mount is refused before execution
- No frame on the wire contains the host's stored credential

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs-cluster` / qfs integration test with a fake driver
- By hand: on the member, `qfs cluster run --commit "insert into /slack-cc-for-qmu/qmu/<channel>/messages/<ts>/replies values (text) ('...')"` and read it back

**Gate** — what must pass before approval:

- The workspace gates in `CLAUDE.md`

## Considerations

- This is "lend the query, not the secret": safer than handing the member a temporary credential. Temporary credential lending is left to a later ticket if the orchestrator needs it.
- Agent-level provenance (an agent the host spawned) is out of scope; the member identity is the unit of trust in this slice.
- The Slack post by hand needs a real Slack account on the host: verification by a person.

## Final Report

Development completed as planned.

### Discovered Insights

- **Insight**: The `qfs` binary may not depend on `qfs-parser` directly (`crates/cmd/tests/dep_direction.rs` pins the binary's dependency set), so borrowed-statement analysis parses through the public `qfs_exec::parse` and walks the serialized AST rather than the typed one.
  **Context**: Walking the serde form (every `segments` path, every `FOLLOW … INTO` target, every `CALL driver.action`) also means a new grammar construct that carries a path is picked up without code changes; constructs that are not mount paths (DDL, `TRANSFORM`, unbound bare names) are refused explicitly.
- **Insight**: Borrowed statements must execute on a fresh OS thread, not `spawn_blocking`.
  **Context**: The commit applier and several drivers build their own tokio runtime and `block_on`, which panics inside a thread that already has the host's runtime context entered.
- **Insight**: The engine's irreversible gate carries over unchanged: a borrowed `REMOVE … --commit` returns `irreversible_ack_required` (exit 4) because the ack flag is never forwarded, and a statement written with a `COMMIT` prefix applies even without `--commit`, exactly as with `qfs run`. The audit's `commit` field records the effective value.
  **Context**: Found during the manual smoke test against a SQLite `/sql/demo` mount.
