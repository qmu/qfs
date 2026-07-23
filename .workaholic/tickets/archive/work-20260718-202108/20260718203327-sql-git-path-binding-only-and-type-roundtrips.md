---
created_at: 2026-07-18T20:33:27+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: []
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# path_binding becomes the only sql/git declaration source; rich column-type round-trips covered

## Overview

The sql and git drivers each stack **three** connection sources today, which is exactly the
declared-connection seam this mission retires. `sql.rs` `conn_registry` / `has_connections`
(`packages/qfs/crates/qfs/src/sql.rs:218-285,356-365`) and `git.rs` `git_driver` /
`has_connections` (`packages/qfs/crates/qfs/src/git.rs:147-229`) each combine
`connections_config::declared_for` (the `connections.qfs` loader,
`packages/qfs/crates/qfs/src/connections_config.rs`), the deprecated `QFS_SQL_*` / `QFS_GIT_*`
environment variables, and the canonical `path_binding` rows
(`path_binding_sql_connections` `sql.rs:292`, `path_binding_git_connections` `git.rs:162`).

Collapse all three to **`path_binding` ONLY** — the re-homed System-DB registry in
`packages/qfs/crates/store/src/schema/system_config_registry.sql`:

- Retire the `connections.qfs` loader and the `QFS_*` env-var fallback per the mission gate: no
  `QFS_*` environment variable is a working path any longer. Experimental, no backward compat — a
  hard break, no deprecation window.
- Repoint `--import-env` to emit `CONNECT /sql/<name> TO … SECRET …` statements rather than
  populate the retired seam.
- **Retire `CREATE CONNECTION`** (`packages/qfs/crates/parser/src/grammar.rs:2718-2742`): it becomes
  a parse error carrying a pointer to `CONNECT` (owner ruling, 2026-07-18). `CONNECT /sql/<name> TO
  <driver> AT… SECRET…` is the one declaration statement.
- Broaden declared-path column-type coverage: round-trip `NUMERIC` / `TIMESTAMP` / `UUID` / `JSON`
  through a `path_binding`-mounted `/sql/<conn>` (`Dialect::map_type` / `sql_type`
  `packages/qfs/crates/…/dialect.rs:102,139`; pg value normalization
  `packages/qfs/crates/…/sql_backends.rs:179-251`).

## Policies

- implementation/one-source-of-truth — sql and git connection registries build from `path_binding`
  alone; there is exactly one place a connection is declared.
- development/no-backward-compat — qfs is experimental; hard breaks are correct, so the
  `connections.qfs` loader and `QFS_*` env fallback are removed outright, not deprecated.

## Quality Gate

1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo fmt --all --check`
4. `cargo run -p xtask -- gen-docs --check`
5. Plugin version bump (taught-surface break: skills/docs mention env vars and `CREATE CONNECTION`)
   — all four plugin `version` fields, minor.
6. Acceptance: sql and git registries build from `path_binding` alone; a `connections.qfs`-only or
   env-var-only configuration produces **no** working mount, recorded in the release note.
7. Acceptance: run / commit / describe all converge on the one `path_binding` registry.
8. Acceptance: the four column types (`NUMERIC`, `TIMESTAMP`, `UUID`, `JSON`) round-trip write→read
   (SQLite hermetic end-to-end; pg/mysql at the value-mapping unit level).
9. Acceptance: `connections_config.rs` and `packages/qfs/crates/core/src/ddl/connections.rs` parse
   seam is removed or reduced to the import-migration helper.
10. Acceptance: `CREATE CONNECTION` now parse-errors with a `CONNECT` pointer (tested).
11. Verification: hermetic qfs-crate tests with `HomeGuard` binding via `db_upsert_binding`,
    asserting registry membership and that env-only / file-only configs bind nothing; a SQLite
    four-type round-trip (`seeded_test_driver` `sql.rs:379`); extended `sql_backends.rs` unit tests.

## Considerations

- Sequencing: items 1 and 3 both touch the System-DB schema area, so land ticket-1's migration
  before or after this cleanly — not interleaved.
- `--import-env` becomes the migration path off the retired env vars, so keep its emitter as the one
  place that still understands the old `QFS_*` shape (emit-only, never a working bind source).
