---
created_at: 2026-07-06T17:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 4h
commit_hash: d1e222d
category: Changed
depends_on: []
---

# git -> path_binding convergence + Postgres NUMERIC/TIMESTAMP/UUID/JSON round-trips

Split from `20260706120100-connect-declared-registry-followups` (Part B, the `--` comments fix,
shipped there). These two are the larger / not-hermetically-verifiable remainder.

## Part A — move `git` onto `path_binding` (like `sql`)

`sql` converged onto the `path_binding` registry (ticket 20260705000500: run + describe converge on
the project DB), but `git` still rides only the older declared-connection seam:
`crates/qfs/src/git.rs::has_connections`/`git_driver` read `connections_config::declared_for("git")`
+ `QFS_GIT_*` env only — zero `path_binding` references. `describe.rs:70-72` documents both as the
"declared-connection seam"; only sql was converged (`describe.rs:294-303`).

- Add `path_binding_git_connections()` mirroring `sql.rs:280-310`'s `path_binding_sql_connections`
  (read `/git/` FULL-connect bindings from the project DB `path_binding` table).
- Wire it into `git_driver()` (register a repo per binding, registered so a persisted binding wins a
  name clash with the env/`connections.qfs` shims) and `has_connections()`.
- Converge describe (so `qfs describe` shows path_binding-connected git repos), matching the sql
  convergence. This is why it is dedicated-ticket-sized.
- Hermetic tests via the project-DB `path_binding` fixtures (as the sql convergence has).

## Part C (Postgres only) — NUMERIC/TIMESTAMP/UUID/JSON value round-trips

`crates/qfs/src/sql_backends.rs::pg_value` decodes BOOL/INT2/4/8/FLOAT4/8/BYTEA; NUMERIC /
TIMESTAMP(TZ) / UUID / JSON(B) fall through to `try_get::<Option<String>>`, which **errors** on those
OIDs. MySQL already round-trips these (its text protocol returns them as `mysql::Value::Bytes`, which
`my_value` decodes to `Text`) — so this is Postgres-only.

- Add postgres feature deps so those OIDs decode: `with-chrono-0_4` (or `with-time`) for
  TIMESTAMP/DATE, `with-uuid-1` for UUID, `with-serde_json-1` for JSON/JSONB, and `rust_decimal`
  (with `db-postgres`) for NUMERIC — then add explicit `pg_value` arms decoding each to `Value::Text`
  (the honest canonical string), matching the `crates/sql-core/src/dialect.rs:102-186` type contract.
- **Verification is live-only**: a `postgres::Row` is not constructible in a hermetic test (only
  bool/int/float/bytea are unit-tested today). Owner runs a live PG round-trip (`SELECT` a NUMERIC /
  TIMESTAMP / UUID / JSON column) as the acceptance, as the original concern's live-verification note
  anticipated.

## Key files

- Part A: `crates/qfs/src/git.rs`, `crates/qfs/src/sql.rs` (the pattern), `crates/qfs/src/describe.rs`.
- Part C: `crates/qfs/src/sql_backends.rs` (`pg_value`), `crates/qfs/Cargo.toml` (postgres features),
  `crates/sql-core/src/dialect.rs` (target contract).

## Considerations

- Source concern: `.workaholic/concerns/11-postgres-mysql-declarations-for-the-declared.md` (its
  already-resolved sub-item — declared PG/MySQL support — stays dropped).
