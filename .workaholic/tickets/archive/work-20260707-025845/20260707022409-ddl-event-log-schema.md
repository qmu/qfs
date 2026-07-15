---
created_at: 2026-07-07T02:24:09+09:00
author: a@qmu.jp
type: enhancement
layer: [DB, Domain]
effort: 0.5h
commit_hash: 624caf8
category: Added
depends_on:
---

# Add Replayable DDL Event Log Schema

## Overview

Add a durable, append-only event log for qfs configuration and DDL changes. The log must sit beside the existing current-state tables in the System DB and record replayable, secret-free event payloads for DDL/config mutations without changing the existing `/sys/audit` metadata-only contract.

## Policies

- `workaholic:planning` / `policies/modeling-centric-design.md` — the feature introduces a state-transition model that must be explicit before code is added.
- `workaholic:design` / `policies/history-structures.md` — the core requirement is preserving configuration transitions as history from the start.
- `workaholic:design` / `policies/data-sovereignty.md` — qfs state must be exportable without exposing credential values.
- `workaholic:implementation` / `policies/directory-structure.md` — schema and model additions must stay in the existing store/qfs layering.
- `workaholic:implementation` / `policies/coding-standards.md` — code changes must follow the local Rust style and compiler-checked patterns.
- `workaholic:implementation` / `policies/persistence.md` — define the SQLite schema first and make the state space explicit.
- `workaholic:implementation` / `policies/type-driven-design.md` — model event kinds, hashes, and payload shape with narrow types rather than raw strings where practical.
- `workaholic:implementation` / `policies/test.md` — schema behavior and hash-chain invariants need unit tests.

## Key Files

- `packages/qfs/crates/store/src/lib.rs` - System DB migration list and migration tests.
- `packages/qfs/crates/store/src/schema/system_audit.sql` - existing hash-chain pattern and metadata-only boundary.
- `packages/qfs/crates/store/src/schema/system_drivers.sql` - current declared-driver state table that DDL events must complement.
- `packages/qfs/crates/store/src/schema/system_policies.sql` - current policy state table.
- `packages/qfs/crates/store/src/schema/system_settings.sql` - current mutable settings table.
- `packages/qfs/crates/store/src/audit.rs` - existing chain model useful as a reference, not a payload store to widen.

## Related History

The existing audit work intentionally stores only metadata and a bounded local tail. This ticket adds a separate replay log so the audit contract remains tight while qfs gains a full configuration-history source.

- [20260627120300-t76-hash-chained-audit-emission.md](.workaholic/tickets/archive/work-20260628-000332/20260627120300-t76-hash-chained-audit-emission.md) - Introduced the metadata-only hash-chained audit stream.
- [20260704145136-declared-driver-surface.md](.workaholic/tickets/archive/work-20260705-032203/20260704145136-declared-driver-surface.md) - Added DDL sugar that writes declared-driver rows into `/sys/drivers`.
- [20260626101100-t53-sys-driver-admin-views.md](.workaholic/tickets/archive/work-20260628-000332/20260626101100-t53-sys-driver-admin-views.md) - Established `/sys` as queryable admin state.

## Implementation Steps

1. Add a new System DB migration for `sys_ddl_events` or an equivalently named table.
2. Include monotonic sequence, transaction/group id, actor, timestamp, normalized target path, verb, source statement text if available, normalized replay payload JSON, previous hash, and current hash.
3. Keep the table secret-free by construction. It may store secret references and auth schemes, but no plaintext credential values.
4. Add a pure event model in the store layer if useful, mirroring the existing audit chain split between pure model and binary-side I/O.
5. Add migration tests proving the table exists, applies idempotently, and shipped migration bodies remain append-only.
6. Add tests for event hash stability and tamper detection if a new event hash model is introduced.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A fresh System DB contains the new DDL event table after migrations.
- The event schema can represent CREATE/CONNECT-style config writes as secret-free replayable payloads.
- Existing `/sys/audit` schema and metadata-only tests remain unchanged in behavior.
- No schema column can reasonably require a plaintext token or passphrase.

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs-store` passes.
- Targeted migration tests assert table existence and idempotency.
- Secret-leak assertions cover debug/rendered event payload examples.

**Gate** — what must pass before approval:

- Store crate tests are green, migration checksum guards remain green, and review confirms the new schema is append-only and secret-free.

## Considerations

- Do not widen `AuditEvent` to carry replay payloads; that would violate the current metadata-only audit boundary (`packages/qfs/crates/store/src/audit.rs`).
- The event payload should be normalized enough for replay even if the original DDL text is omitted or unavailable (`packages/qfs/crates/parser/src/grammar.rs`).
- Schema naming should leave room for project-scoped events later if project DB state needs the same replay behavior (`packages/qfs/crates/store/src/schema/project_path_bindings.sql`).

## Final Report

Implemented a new System DB migration for `sys_ddl_events` and a pure store-layer DDL event model with stable content hashes, hash-chain links, and verification helpers. The schema records monotonic sequence, grouping, actor/time, target path, verb, optional source text, normalized replay payload JSON, and hash-chain fields while keeping replay payloads separate from `/sys/audit`.

Verification:

- `cargo fmt --all --check`
- `cargo test -p qfs-store`
- `cargo run -p xtask -- check-migrations`

Concerns:

- The event writer is still future work, so callers must continue enforcing secret-free normalized payloads when ticket `20260707022410-record-ddl-events-on-config-writes.md` is implemented.
- This migration covers System DB configuration history; project-scoped replay logs remain a later extension if project DB DDL needs equivalent behavior.
