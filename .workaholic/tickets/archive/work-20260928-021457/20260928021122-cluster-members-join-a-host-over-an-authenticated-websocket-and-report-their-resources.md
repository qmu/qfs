---
created_at: 2026-09-28T02:11:22+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools
merge_policy:
verification_handoff: 
---

# Cluster members join a host over an authenticated WebSocket and report their resources

## Overview

First slice of the cluster (feedback `20260928020955-...`). A qfs instance started as a
cluster **host** accepts **members** over a persistent WebSocket; each member authenticates
with a join token the host issued, then reports a heartbeat carrying its hostname, CPU,
memory, disk and its Claude Code sessions (`/claude/sessions` rows). The host exposes the
aggregate as queryable paths `/cluster/members` and `/cluster/sessions`. Security first: the
trust model is written as an ADR before code relies on it.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:design` — security design: default deny, least privilege, secrets never logged

## Key Files

- `packages/qfs/crates/cluster/` (new) — frame protocol, join-token check, member registry, resource sampling
- `packages/qfs/crates/qfs/src/cluster.rs` (new) — CLI wiring and the `/cluster` read driver
- `packages/qfs/crates/cmd/` — `qfs cluster host|join|token` subcommands
- `packages/qfs/crates/qfs/src/claude.rs` — source of the member's session rows
- `docs/adr/` — new ADR: cluster trust model

## Implementation Steps

1. Write the ADR: host holds a random cluster secret; `qfs cluster token` mints a member join token (member name + expiry, HMAC-SHA256 over the secret); member presents it in the WebSocket handshake; host rejects unknown/expired/replayed tokens; host binds loopback by default and needs `--listen` to expose; recommend TLS/SSH tunnel beyond a trusted LAN (MVP: plain ws on LAN, stated as a limit).
2. Add the `qfs-cluster` crate with `tokio-tungstenite`: JSON frames `hello`, `heartbeat`, `request`, `response`.
3. `qfs cluster host --listen <addr>` runs the accept loop and an in-memory registry (members are stateless; the host is the only state).
4. `qfs cluster join <ws-url> --token <t> --name <n>` connects, reconnects with backoff, and sends a heartbeat every 10 s (CPU/mem/disk via `sysinfo`, sessions via the existing claude reader).
5. Expose `/cluster/members` (name, addr, cpu_pct, mem_used, mem_total, disk_used, disk_total, last_seen, status) and `/cluster/sessions` (member, id, cwd, status, last_message, ts) on the host.
6. Hermetic tests: token accept/reject, heartbeat aggregation over an in-process host+member pair.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A member with a valid token joins and appears in `/cluster/members` with non-zero memory and disk totals
- An expired or tampered token is refused and nothing is registered
- A member that disconnects shows `status = offline` after its heartbeat lapses

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs-cluster` and the in-process host/member test
- By hand: two terminals, `qfs cluster host` + `qfs cluster join`, then `qfs cluster members`

**Gate** — what must pass before approval:

- The workspace gates in `CLAUDE.md` (test, clippy, fmt, gen-docs --check)

## Considerations

- WebSocket is the developer's stated transport; the in-house HTTP server has no upgrade path, so the cluster listener is its own port rather than a route on `qfs serve`. Folding it into `qfs serve` is later work.
- `crates/tunnel` (relay fabric) overlaps in intent; reuse is deferred to keep this slice small.
- Plain `ws://` is an MVP limit, named in the ADR.

## Final Report

Development completed as planned.

### Discovered Insights

- **Insight**: `/cluster/members` and `/cluster/sessions` are exposed as the host's loopback JSON API and the `qfs cluster members|sessions` subcommands, not yet as `qfs run "/cluster/..."` driver paths.
  **Context**: A `qfs run` one-shot is a separate process from the long-running `qfs cluster host`, so a read driver would have to fetch from the host listener anyway; the JSON API is that seam, and a thin `/cluster` read driver over it is a small follow-up.
- **Insight**: Creating the cluster secret must be atomic — a `qfs cluster host` and a `qfs cluster token` started together raced on first use during the manual smoke test.
  **Context**: The secret is now written to a private temp file and hard-linked into place; the loser of the race reads the winner's secret.
- **Insight**: The repository had no live `docs/adr/` directory (the old qfs ADRs 0001–0007 were removed from `packages/qfs/docs/adr/`), so the trust-model ADR is `docs/adr/0008-cluster-trust-model.md`, continuing that numbering.
  **Context**: Avoids reusing a number that historical code comments still cite (e.g. ADR-0005, ADR-0007).
