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
   after three missed heartbeats. Members accept no inbound commands: a `request` frame sent *to*
   a member is answered with a refusal (execution only flows member → host).
6. **Lend the query, not the secret.** A member sends a *statement* (`request` frame) from
   member to host; the host runs it against its own mounts and returns the result envelope
   (`response` frame). Credentials stay inside the host's engine: a frame carries only the
   statement and its result rows or error. See "Borrowed execution" below.
7. **Audit.** Admissions, refusals (with reason, never the token) and disconnects are logged by the
   host. Every borrowed request — executed, failed, or refused — is appended to
   `<config>/cluster/audit.log` as one JSON line: `ts`, `member`, `statement`, `commit` (effective,
   including a `COMMIT <stmt>` wrapper), `outcome` (`ok` / `error` / `refused`) and, when not `ok`,
   the `detail` envelope.

### Borrowed execution

- **Grant table, default deny.** `qfs cluster grant <member> <mount>` / `revoke` / `grants` edit
  `<config>/cluster/grants.json` on the host. A grant is a path prefix compared segment by segment:
  `/slack-acct` admits everything under that mount, `/sql/demo` only that connection (and not
  `/sql/other`, nor a sibling `/slack-acctx`). The table is re-read on every request, so a revoke
  takes effect without restarting the host; a corrupt table refuses (never reads as "allow").
- **Authorize before executing.** The host parses the statement on the shipped grammar and lists
  every path it touches (every path expression, `FOLLOW … INTO` target, and `CALL <driver>.…`).
  Every one must be under a grant for that member, or the request is refused before anything runs.
  Fail closed: an unparsable statement, one with no path, server DDL (`CREATE …` — a member must
  not define bindings on the host), a `TRANSFORM` stage (it spends the host's model provider), and
  a bare-name source not bound by a `LET` in the same statement are all refused.
- **Same engine, same gates.** An authorized statement runs through the one-shot path `qfs run`
  uses (the live run context, the real commit applier, the host's safety mode). Preview unless the
  member passes `--commit`. The irreversible acknowledgement is never forwarded: an irreversible
  effect is refused exactly as `qfs run --commit` without `--commit-irreversible` refuses it.
- **Authentication.** A request is honoured only on a connection whose `hello` verified. `qfs
  cluster run` opens a one-shot connection with the member's join token and an `exec_only` hello:
  it is authenticated like a member but never registered, so it does not disturb the joined
  member's liveness row. A request before a valid hello is answered with `refused` and the socket
  closes.
- **Not in this slice.** Agent-level provenance (the member is the unit of trust), temporary
  credential lending, and TLS.

## Consequences

- A leaked token admits one named member until it expires; it grants no host credential, and it
  can execute only what the host granted that member. A leaked cluster secret admits anything — it is guarded like the
  credential vault.
- Anyone on the LAN path can read frames while the transport is plain `ws://`; this is stated, not
  hidden, and is why exposure requires an explicit `--listen`.
- The loopback-only read API means `qfs cluster members|sessions` are host-machine commands.
