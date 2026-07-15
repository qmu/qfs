---
created_at: 2026-07-07T02:24:11+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure, Config]
effort: 1h
commit_hash: 1e3da91
category: Added
depends_on: [20260707022409-ddl-event-log-schema.md]
---

# Add Secret-Free qfs State Dump

## Overview

Add a MySQL-like qfs state dump that exports current qfs configuration as replayable qfs statements or structured JSONL. The dump should be secret-free, deterministic, and useful for backup, review, and migration between hosts.

## Policies

- `workaholic:planning` / `policies/modeling-centric-design.md` — the dump format must reflect the qfs state model, not incidental SQLite layout.
- `workaholic:design` / `policies/data-sovereignty.md` — qfs operators need an explicit export path for their own configuration data.
- `workaholic:design` / `policies/history-structures.md` — dumps should include enough audit/event metadata to relate a snapshot to its history.
- `workaholic:implementation` / `policies/directory-structure.md` — CLI and serialization code must live in the existing binary/store boundaries.
- `workaholic:implementation` / `policies/coding-standards.md` — output must be deterministic and covered by tests.
- `workaholic:implementation` / `policies/objective-documentation.md` — document exactly what is dumped and what is intentionally excluded.
- `workaholic:operation` / `policies/ci-cd.md` — dump/restore checks should be runnable locally as part of the normal test gate.

## Key Files

- `packages/qfs/crates/cmd/src/lib.rs` - CLI surface for adding `qfs dump` or equivalent.
- `packages/qfs/crates/qfs/src/sys.rs` - current `/sys` state reader.
- `packages/qfs/crates/qfs/src/path_binding.rs` - project path bindings to include as `CONNECT` statements or JSON.
- `packages/qfs/crates/store/src/schema/system_drivers.sql` - declared-driver current-state rows.
- `packages/qfs/crates/store/src/schema/system_settings.sql` - system settings current-state rows.
- `docs/guide/cli.md` - operator-facing command documentation.

## Related History

qfs already treats admin state as queryable data and DDL as sugar over writes. This ticket exposes that state in a deterministic backup/export surface.

- [20260626101100-t53-sys-driver-admin-views.md](.workaholic/tickets/archive/work-20260628-000332/20260626101100-t53-sys-driver-admin-views.md) - `/sys` admin data became queryable.
- [20260704145136-declared-driver-surface.md](.workaholic/tickets/archive/work-20260705-032203/20260704145136-declared-driver-surface.md) - Declared-driver rows became durable system state.
- [20260701100020-defined-path-declaration-grammar-config.md](.workaholic/tickets/archive/work-20260629-110121/20260701100020-defined-path-declaration-grammar-config.md) - Defined paths became declarative configuration.

## Implementation Steps

1. Define dump modes: current snapshot only, and optionally current snapshot plus event-log metadata when ticket `20260707022410` is implemented.
2. Add a CLI command such as `qfs dump [--format qfs|jsonl] [--include-events]`.
3. Emit deterministic ordering for drivers, types, views, maps, settings, policies, and path bindings.
4. Render secret references and account labels only; never render plaintext secrets or passphrases.
5. Include a header with qfs version, schema version, generated timestamp, and audit/event chain head if available.
6. Add documentation that explains what the dump can restore and what still requires encrypted vault backup.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Dumping a fixture System DB produces stable output across repeated runs, ignoring only the generated timestamp if present.
- Declared-driver state round-trips into readable qfs statements or structured JSON records.
- Path bindings render `CONNECT` statements or equivalent records without secret values.
- Secret canaries in vault/project tables do not appear in dump output.
- The dump clearly marks credential references that require separate vault restoration.

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs -p qfs-cmd` passes.
- Golden tests cover qfs-format and/or JSONL-format dumps.
- Secret canary tests assert the output excludes token-like values.
- Docs mention the backup boundary between qfs state and encrypted credential material.

**Gate** — what must pass before approval:

- CLI tests are green, dump output is deterministic, and a reviewer can inspect a fixture dump without seeing secrets.

## Considerations

- Prefer qfs statements for human review, but JSONL may be better for full fidelity; if both cannot fit, implement one format and document the tradeoff (`packages/qfs/crates/cmd/src/lib.rs`).
- Dump should use public state readers where practical instead of reaching around abstractions into arbitrary SQLite queries (`packages/qfs/crates/qfs/src/sys.rs`).
- Restoring secrets is out of scope; do not invent plaintext secret export.

## Final Report

Implemented `qfs dump --format jsonl [--include-events]` as a secret-free JSONL export surface. The command is parsed in `qfs-cmd` and dispatched through a binary-injected dump launcher; the binary reads the System/Project DBs and emits a deterministic header plus current-state rows for declared drivers, settings, policies, billing labels, and path bindings. `--include-events` appends `sys_ddl_events` rows after the current snapshot.

The first shipped format is JSONL rather than qfs statements because it preserves structured fidelity for backup/review without inventing restore semantics. The dump deliberately excludes encrypted credential-store value columns and documents that vault material must be backed up separately.

Verification:

- `cargo fmt --all --check`
- `cargo test -p qfs-cmd`
- `cargo test -p qfs`

Concerns:

- Restore/replay is still a separate ticket; this is an export/read surface only.
- qfs-statement rendering remains future work if operators need a more hand-editable dump format.
