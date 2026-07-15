---
created_at: 2026-07-07T02:24:12+09:00
author: a@qmu.jp
type: enhancement
layer: [DB, Domain, Infrastructure, Config]
effort: 1h
commit_hash: 137d1ac
category: Changed
depends_on: [20260707022410-record-ddl-events-on-config-writes.md, 20260707022411-dump-current-qfs-state.md]
---

# Restore and Replay qfs State Dumps

## Overview

Add a restore path for qfs state dumps. Restoring should replay dumped statements or event records through the same preview/commit machinery as normal qfs writes, so restored state observes the same validation, policy, audit, and secret-free boundaries as live configuration changes.

## Policies

- `workaholic:planning` / `policies/modeling-centric-design.md` — replay semantics must match the qfs state model rather than raw SQLite import.
- `workaholic:design` / `policies/data-sovereignty.md` — operators need a practical way to recover or move their qfs state.
- `workaholic:design` / `policies/history-structures.md` — event replay should preserve or intentionally re-anchor history, with the behavior explicit.
- `workaholic:implementation` / `policies/directory-structure.md` — restore logic must use existing parser/commit paths rather than direct SQL writes where possible.
- `workaholic:implementation` / `policies/coding-standards.md` — restore errors must be structured, stable, and secret-free.
- `workaholic:implementation` / `policies/test.md` — dump-to-restore round trips require fixture tests.
- `workaholic:operation` / `policies/ci-cd.md` — recovery behavior must be reproducible locally.

## Key Files

- `packages/qfs/crates/cmd/src/lib.rs` - CLI surface for `qfs restore`.
- `packages/qfs/crates/exec/src/lib.rs` - one-shot execution path and preview/commit behavior.
- `packages/qfs/crates/qfs/src/commit.rs` - audit emission and commit routing.
- `packages/qfs/crates/qfs/src/sys.rs` - System DB-backed config apply paths.
- `packages/qfs/crates/qfs/src/path_binding.rs` - project path-binding restore target.
- `docs/guide/cli.md` - operator-facing restore documentation.

## Related History

The existing design favors replay through ordinary qfs statements rather than privileged loaders. Restore should follow that pattern so backup recovery is just another controlled write path.

- [20260622214650-t09-effect-plan-and-preview-commit.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t09-effect-plan-and-preview-commit.md) - Established preview/commit as the write gate.
- [20260622214650-t30-server-runtime-and-self-config-driver.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t30-server-runtime-and-self-config-driver.md) - Server boot replays config through the same mutation path.
- [20260627120300-t76-hash-chained-audit-emission.md](.workaholic/tickets/archive/work-20260628-000332/20260627120300-t76-hash-chained-audit-emission.md) - Existing audit chain must remain the metadata record of restore commits.

## Implementation Steps

1. Define restore modes: preview-only by default, commit on explicit `--commit`, and an optional event-preserving mode if event-log dumps contain enough information.
2. Parse qfs-format dumps as ordinary statements and JSONL dumps as normalized event records lowered to ordinary writes.
3. Reuse existing preview/commit machinery; avoid direct SQL import except for a clearly documented low-level recovery mode.
4. Detect missing credential material and report actionable, secret-free errors that explain which account/vault reference must be restored separately.
5. Add idempotency behavior: replaying the same current-state dump should converge via UPSERT/CONNECT semantics rather than duplicate rows where possible.
6. Document how restore affects audit/event history: whether restored events are re-recorded as new local events, imported as historical events, or both.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `qfs restore` previews by default and does not mutate state without explicit commit.
- Restoring a dump created by `qfs dump` recreates declared drivers, settings, policies, and path bindings in a fresh test store.
- Re-running the same restore is idempotent or fails with a documented duplicate error where idempotency is not possible.
- Restore never requires or prints plaintext secrets.
- Restore emits normal audit/DDL events for committed changes.

**Verification method** — the commands/tests/probes that prove them:

- `cargo test -p qfs -p qfs-cmd -p qfs-exec` passes.
- End-to-end fixture test performs dump, fresh-store restore preview, restore commit, and state comparison.
- Negative tests cover missing vault references and malformed dump records.

**Gate** — what must pass before approval:

- End-to-end dump/restore tests are green and documentation states the secret-backup boundary and history semantics precisely.

## Considerations

- Restoring by raw SQLite import would bypass qfs validation and should remain out of the normal path (`packages/qfs/crates/exec/src/lib.rs`).
- Imported historical event hashes may not fit the local audit chain; decide explicitly whether to preserve them as external provenance or re-anchor them as local events (`packages/qfs/crates/store/src/schema/system_audit.sql`).
- The restore surface must fail closed on unknown dump versions.

## Final Report

Implemented `qfs restore <dump.jsonl> [--commit]` for the JSONL format produced by `qfs dump`. Restore previews by default, validates the `qfs-state-jsonl-v1` header, and only mutates state with `--commit`. Committed restores replay supported current-state records for settings, billing labels, policies, declared drivers, and path bindings; System DB-backed writes go through the `/sys` backend so they emit fresh local audit/DDL events. Existing driver/policy rows are detected and skipped so re-running the same restore converges instead of duplicating those append-style rows.

Dumped historical `ddl_event` records are intentionally not imported into the local hash chain. They are counted as skipped provenance; committed restore re-anchors history by recording new local events for the writes it applies. Credential values remain out of scope: restore rebuilds references such as `vault:provider/account`, while encrypted vault material must be restored separately.

Verification:

- `cargo fmt --all --check`
- `cargo test -p qfs-cmd`
- `cargo test -p qfs-exec`
- `cargo test -p qfs`

Concerns:

- Restore currently supports the shipped JSONL format only, not qfs-statement dumps.
- Path bindings are restored through the Project DB binding helper because `/sys/paths` still cannot share the System DB event-log transaction.
