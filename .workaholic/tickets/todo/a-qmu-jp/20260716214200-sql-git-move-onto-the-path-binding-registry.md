---
created_at: 2026-07-16T21:42:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort:
commit_hash:
category:
depends_on:
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# Move sql/git onto the path_binding registry, and cover the declared-path column types

## Overview

Mission acceptance item 3 (concern `postgres-mysql-declarations-for-the-declared`). Two gaps,
both verified against source this session:

**Gap 1 — `CREATE CONNECTION` still lives in a config file, not the registry.** Three stacked
registration sources exist for `/sql/<conn>` and `/git/<repo>`, oldest→newest so the newest wins
a name clash:

1. The `QFS_SQL_*` / `QFS_GIT_*` env vars (`sql.rs` env loop; `git.rs:205`) — already a warned,
   deprecated fallback.
2. **The file-backed declared-connection seam**: `CREATE CONNECTION` rows never land in any DB —
   they are re-parsed on every load from `connections.qfs` (`QFS_CONNECTIONS` env, else
   `~/.config/qfs/connections.qfs`) by `connections_config.rs` (`declared_connections()` :23,
   `declared_for()` :49), with the parse in `core/src/ddl/connections.rs`
   (`DeclaredConnection { name, driver, at_locator, secret_ref }`). Consumed at `sql.rs:248`
   (postgres), `sql.rs:257` (mysql), `git.rs:196` (git). SQLite is asymmetric: declared SQLite
   connections ride the env-var block.
3. **The `path_binding` registry (canonical, System DB after 20260716143641)**: `sql.rs:272` /
   `sql.rs:292 path_binding_sql_connections()` and `git.rs:224` / `git.rs:162
   path_binding_git_connections()` — registered LAST so a persisted binding wins.

So the same logical intent has two incompatible homes: `CONNECT /sql/shop TO sqlite …` writes a
ledgered System-DB row; `CREATE CONNECTION shop DRIVER sqlite …` writes a line in a config file
that is invisible to `/sys/paths`, to `qfs dump`/`restore`, to provisioning, and to the DDL event
log. The cloud mounts and declared REST drivers already read `path_binding` only — sql/git are
the last riders on the old seam.

**Gap 2 — declared-path column types map in the catalog but degrade on read, untested.**
`Dialect::map_type()` (`sql-core/src/dialect.rs:102`) maps `numeric→Decimal`,
`timestamp→Timestamp`, `uuid→Uuid`, `json→Json`, and `Dialect::sql_type()` (:138) emits the
right DDL spellings — but the value decode stringifies all four: `pg_value()`
(`sql_backends.rs:228/234/237/240/243`) and `my_value()` (:534) return `Value::Text` for
NUMERIC / TIMESTAMP / UUID / JSON. No round-trip test exists for a value of any of these types
written through a declared `/sql/<conn>` mount and read back (existing tests cover int/text
only: `driver-sql/src/tests.rs:1053`; the DDL emit tests assert SQL strings only).

## Implementation Steps

1. **Route `CREATE CONNECTION` for sqlite/postgres/mysql/git into `path_binding`.** The DDL's
   apply lands a ledgered System-DB row via the shared transactional writer
   (`sys.rs::ledgered_paths_write_tx` + `path_binding::db_upsert_binding`) at the canonical
   mount path (`/sql/<name>` / `/git/<name>`) — the same home `CONNECT` writes. Re-declaring
   replaces (upsert-on-path), matching replace-on-install semantics.
2. **Repoint the sql/git loaders** off `connections_config::declared_for()` onto the (already
   existing) `path_binding` loops, making the registry the single source. Fold the SQLite
   asymmetry: declared SQLite connections come from the registry like postgres/mysql, not the
   env block.
3. **Demote the file seam.** `connections.qfs` becomes a warned, deprecated fallback exactly like
   the env vars (or a one-shot `--import-env`-style migration into `path_binding` — mirror
   whichever the env-var path already does), and `connections_config.rs` shrinks to that shim.
   Hard break is acceptable; qfs is pre-release.
4. **Column-type round-trips.** Add declared-path tests: for each of NUMERIC/DECIMAL,
   TIMESTAMP(TZ), UUID, JSON(B) — create a table through the `/sql/<conn>` catalog write, INSERT
   a typed value, SELECT it back, and assert what comes back. Decide and pin the policy: either
   the decode preserves the canonical type (extend `pg_value`/`my_value`) or Text-degradation is
   the documented contract (then the test pins the Text form and `describe` must not promise
   more than the decode delivers). SQLite collapses to TEXT by dialect — pin that too.
5. Docs: whatever `gen-docs --check` says moves; the drivers/language pages re-render from the
   binary.

## Key Files

- `packages/qfs/crates/qfs/src/connections_config.rs` — the seam this ticket retires to a shim.
- `packages/qfs/crates/core/src/ddl/connections.rs` — the CREATE CONNECTION parse (stays; its
  apply target changes).
- `packages/qfs/crates/qfs/src/sql.rs:240-300`, `git.rs:150-230` — the three stacked loaders.
- `packages/qfs/crates/qfs/src/sys.rs` — `ledgered_paths_write_tx` (the write seam to reuse).
- `packages/qfs/crates/sql-core/src/dialect.rs:102,138` — type mapping both directions.
- `packages/qfs/crates/qfs/src/sql_backends.rs:201-250,534` — the value decodes to extend or pin.
- `packages/qfs/crates/driver-sql/src/tests.rs` — where the round-trip tests live.

## Policies

- `workaholic:implementation` / `anti-corruption-structure` — one registry for one concept;
  the config-file fork is the corruption this closes.
- `workaholic:implementation` / `persistence` — the registry write rides the ledgered
  transaction; the file shim never silently diverges (warned like the env vars).
- `workaholic:implementation` / `type-driven-design` — the column-type contract becomes what the
  decode actually delivers, pinned by tests, not a catalog promise the value layer breaks.
- `workaholic:implementation` / `coding-standards` + `test`.

## Quality Gate

1. `CREATE CONNECTION shop DRIVER sqlite AT '<db>'` followed by a read of `/sql/shop/<table>`
   works with **no** `connections.qfs` file and **no** env var; `/sys/paths` lists the binding;
   the DDL event log carries the write. Same for postgres/mysql (hermetic: connection-string
   level) and git.
2. Both-directions: a test asserting the `CREATE CONNECTION` row lands in `path_binding` fails
   on current code (where it lands in no store at all) before the fix.
3. `connections.qfs` content still mounts (shim) but warns once, like `QFS_SQL_*`.
4. The four column-type round-trips are pinned per dialect (postgres/mysql hermetic at the
   decode level with the real wire types; sqlite live in-process).
5. Baseline gates (`CLAUDE.md`): workspace tests, clippy, fmt, gen-docs/gen-skills --check,
   check-migrations, patch bump on the shipped PR.

## Considerations

- Do NOT bundle with the cloud-account-declarations ticket (20260716214100): both write
  `path_binding`-adjacent state but are independent; whichever lands second rebases trivially.
- `git` bindings carry no SECRET; sqlite carries none either — only postgres/mysql have
  `secret_ref`. The registry row shape already covers all four.
- The `connections.qfs` file on the operator's box (if any) must keep working through the shim
  until the drop; check `QFS_CONNECTIONS` usage in the job/server tests before demoting.
