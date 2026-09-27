# ADR 0008 — Cluster trust model: HMAC join tokens, loopback by default, lend the query not the secret

- **Status**: Accepted
- **Date**: 2026-09-28
- **Deciders**: a@qmu.jp (developer), implemented by the cluster mission
- **Ticket**: `20260928021122-cluster-members-join-a-host-over-an-authenticated-websocket-and-report-their-resources`
  (mission `a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools`)
- **Supersedes / superseded by**: none
- **References**: `packages/qfs/crates/cluster/` (protocol, token, registry, listener),
  `packages/qfs/crates/qfs/src/cluster.rs` (secret persistence, CLI wiring), `docs/security/threat-model.md`

(ADR numbers 0001–0007 were used by the qfs ADRs formerly under `packages/qfs/docs/adr/`; this
continues that sequence.)

## Context

A qfs **cluster** lets several machines (members) attach to one qfs (the host) so the host can see
their resources and Claude Code sessions, and — in the next ticket — so a member can run a
statement against a mount whose credential lives only on the host. Any network-facing surface that
aggregates machine state and will later execute statements must have its trust model fixed before
code relies on it.

## Decision

1. **The host is the only trust anchor.** On first use the host generates a random 32-byte
   *cluster secret* and stores it as `<qfs config dir>/cluster/secret` (mode `0600`, created
   atomically; `--state-dir` overrides the directory). The secret never leaves the host, is never
   printed, and never travels in a frame.
2. **Join tokens.** `qfs cluster token --name <member> [--ttl 24h]`, run on the host, mints
   `base64url(payload) "." base64url(HMAC-SHA256(secret, payload))` with payload
   `{member, exp, nonce}`. A member presents it in its first WebSocket frame (`hello`). The host
   refuses — and registers nothing — when the signature does not verify (tampered or foreign
   token), the token is expired, or the presented name differs from the token's `member`.
   Verification is constant-time. A token is a bearer credential for its lifetime: the member reuses
   it to reconnect, so a short TTL is the revocation mechanism in this slice (rotating the secret
   file revokes every token at once). Single-use/replay tracking is **not** implemented; a newer
   connection under the same name replaces the older registry entry.
3. **Loopback by default.** `qfs cluster host` binds `127.0.0.1:7466` unless `--listen` says
   otherwise, and prints a warning when bound beyond loopback. The JSON read API on the same port
   (`GET /api/cluster/members`, `/api/cluster/sessions`, and the later GUI) answers **only peers
   whose address is loopback**, whatever the bind address; a remote peer gets `403`. Only the
   token-authenticated `/cluster/ws` upgrade is reachable from the network.
4. **Plain `ws://` is an MVP limit.** Frames — including the join token in `hello` and the
   members' session summaries — travel unencrypted. Use the cluster only on a trusted LAN, or put
   the listener behind an SSH tunnel / TLS-terminating proxy. Native TLS (`wss://`) is later work.
5. **Members are stateless reporters.** A member sends hostname, CPU, memory, disk and its live
   Claude Code session rows (id, cwd, name, status, last visible message — the `/claude/sessions`
   surface, not transcripts). The host keeps them in memory only; a member lapses to `offline`
   after three missed heartbeats. Members accept no inbound commands in this slice: a `request`
   frame is answered with a refusal.
6. **Lend the query, not the secret** (next ticket). Borrowed execution will send a *statement*
   from member to host; the host runs it against its own mounts and returns rows. Credentials stay
   on the host. The host executes only mounts it has explicitly **granted** to that member
   (default: nothing), checked before execution.
7. **Audit.** Admissions, refusals (with reason, never the token) and disconnects are logged by the
   host. Every borrowed execution (next ticket) is written to the host's audit log with member,
   statement and outcome.

## Consequences

- A leaked token admits one named member until it expires; it grants no host credential and (in
  this slice) no execution. A leaked cluster secret admits anything — it is guarded like the
  credential vault.
- Anyone on the LAN path can read frames while the transport is plain `ws://`; this is stated, not
  hidden, and is why exposure requires an explicit `--listen`.
- The loopback-only read API means `qfs cluster members|sessions` are host-machine commands.
