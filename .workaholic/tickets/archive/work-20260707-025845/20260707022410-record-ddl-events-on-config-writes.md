---
created_at: 2026-07-07T02:24:10+09:00
author: a@qmu.jp
type: enhancement
layer: [DB, Domain, Config]
effort: 1h
commit_hash: 3385eb3
category: Changed
depends_on: [20260707022409-ddl-event-log-schema.md]
---

# Record DDL Events When Config State Changes

## Overview

Wire the DDL event log into every committed qfs configuration mutation. DDL and config writes should update the current-state snapshot and append a replayable event in the same transaction wherever both records live in the same SQLite database.

## Policies

- `workaholic:planning` / `policies/modeling-centric-design.md` — the event and snapshot models must stay aligned as one coherent state model.
- `workaholic:design` / `policies/history-structures.md` — every configuration transition should be preserved as history.
- `workaholic:design` / `policies/data-sovereignty.md` — recorded events must support later export without credential leakage.
- `workaholic:implementation` / `policies/directory-structure.md` — write-path changes must stay in the existing sys/server/path binding modules.
- `workaholic:implementation` / `policies/coding-standards.md` — implementation should remain small, typed, and panic-free.
- `workaholic:implementation` / `policies/domain-layer-separation.md` — event construction should be separated from CLI and storage entry points.
- `workaholic:implementation` / `policies/test.md` — each write path needs tests proving snapshot and event append stay consistent.

## Key Files

- `packages/qfs/crates/qfs/src/sys.rs` - `/sys` admin write paths for policies, settings, drivers, accounts, paths, and billing.
- `packages/qfs/crates/qfs/src/path_binding.rs` - canonical defined-path writes for `qfs connect`.
- `packages/qfs/crates/qfs/src/connection.rs` - CLI connect entry point over path bindings.
- `packages/qfs/crates/parser/src/grammar.rs` - DDL desugars to ordinary effect writes.
- `packages/qfs/crates/server/src/runtime.rs` - `/server` DDL boot and hot-reconfigure apply path.
- `packages/qfs/crates/server/src/audit.rs` - existing in-memory server config audit sink.

## Related History

The parser already lowers DDL to ordinary writes, and `/sys` mutations already self-audit. This ticket adds a second, replayable event write alongside those current-state mutations.

- [20260704145136-declared-driver-surface.md](.workaholic/tickets/archive/work-20260705-032203/20260704145136-declared-driver-surface.md) - Parser desugars declared-driver DDL into `/sys/drivers` rows.
- [20260703040000-create-account-language-surface.md](.workaholic/tickets/archive/work-20260705-173620/20260703040000-create-account-language-surface.md) - Added `CREATE ACCOUNT` as language sugar over `/sys/accounts`.
- [20260701100000-epic-defined-paths-replace-driver-mounts.md](.workaholic/tickets/archive/work-20260629-110121/20260701100000-epic-defined-paths-replace-driver-mounts.md) - Established path bindings as current-state source of truth.

## Implementation Steps

1. Add a binary-side append helper that accepts the normalized target path, verb, replay payload, optional source text, actor, and transaction id.
2. Call the helper inside existing `/sys` transactions for drivers, policies, settings, billing, and other System DB-backed config writes.
3. For project-scoped config such as `path_binding`, either add a project-scoped event log or explicitly record the limitation and only add event logging where atomicity is possible.
4. Decide how `/server` in-memory config writes should persist events: either attach them to System DB-backed runtime state or leave a documented follow-up if `/server` still has no durable registry.
5. Ensure DDL event append never records row payloads that contain secret values; only normalized secret references are allowed.
6. Keep existing `/sys/audit` emission intact, with the DDL event log as a separate replay source.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Committing `CREATE DRIVER` appends a replayable DDL event and still inserts into `sys_drivers`.
- Committing policy/settings writes appends events transactionally with the current-state row update.
- Failed commits do not append successful replay events.
- Existing audit events still appear exactly as metadata-only events.
- No recorded event contains a plaintext token canary.

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs` passes.
- Targeted tests in `sys.rs` or adjacent modules assert state row plus event row in one transaction.
- Negative tests assert malformed writes do not append events.
- Secret canary tests mirror existing `/sys/connections` and audit redaction checks.

**Gate** — what must pass before approval:

- qfs binary tests and store tests are green, and at least one DDL path plus one non-DDL config path prove event recording end to end.

## Considerations

- Cross-DB atomicity is not available between System DB and Project DB; do not pretend project path-binding events are atomic unless they live in the same database (`packages/qfs/crates/qfs/src/sys.rs`).
- The implementation should reuse the parser's normalized effect data where possible rather than reparsing DDL text (`packages/qfs/crates/parser/src/grammar.rs`).
- The event log must not become an authorization bypass; normal preview/commit and policy gates still apply.

## Final Report

Implemented transactional DDL/config event recording for the System DB-backed `/sys` mutation paths. Policy inserts, settings upserts, billing upserts, provider billing events, and declared-driver inserts now append a hash-chained row to `sys_ddl_events` in the same SQLite transaction as the current-state row and existing metadata-only audit row.

The event payload builder records normalized JSON for replay and redacts secret-like keys, including a regression test where a setting named `api_token` does not persist a plaintext token canary. Malformed policy writes still fail before appending any DDL event. Project DB-backed paths/accounts remain outside this ticket because they cannot atomically share the System DB event-log transaction.

Verification:

- `cargo fmt --all --check`
- `cargo test -p qfs sys::tests`
- `cargo test -p qfs`
- `cargo test -p qfs-store`

Concerns:

- `source_text` is still `None` because the current `/sys` backend receives normalized effect rows, not the original statement text.
- `/sys/paths`, `/sys/accounts`, and `/server` in-memory configuration need a separate design for durable event recording because their state is not committed in the same System DB transaction.
