---
created_at: 2026-07-06T12:01:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 4h
commit_hash: 11b910f
category: Changed
depends_on: []
---

# CONNECT-epic declared-registry follow-ups: git -> path_binding, connections comments, SQL value round-trips

Three verified-open follow-ups from the CONNECT epic. They are independent (may be split into
three tickets) but all are declared-registry / SQL plumbing.

## Scope delivered here (narrowed during implementation)

Only **Part B** (the `--` comments fix) ships in this ticket — the one clean, hermetic,
dependency-free fix. The other two are legitimately larger / not hermetically verifiable, and the
ticket itself invited splitting:

- **Part A (git → path_binding)** is a *dedicated-ticket-sized* convergence — the identical `sql`
  work was its own ticket (20260705000500: run + describe converge). Split to a follow-up.
- **Part C** — investigation found **MySQL already round-trips**: its text protocol returns
  NUMERIC/TIMESTAMP/JSON as `mysql::Value::Bytes`, which `my_value` already decodes to `Text`. Only
  **Postgres** remains, and it needs postgres **feature deps** (with-chrono / with-uuid /
  with-serde_json + rust_decimal) to decode those OIDs, plus a **live Postgres** to verify (a
  `postgres::Row` is not constructible in a hermetic test — only bool/int/float/bytea are unit-tested
  today). Split to a follow-up.

Follow-up ticket: `20260706170000-git-path-binding-and-pg-value-round-trips`.

## Part A — move `git` onto `path_binding` (like `sql`)

`sql` converged onto the `path_binding` registry (ticket 20260705000500,
`crates/qfs/src/sql.rs:265-310`), but `git` still rides only the older declared-connection seam:
`crates/qfs/src/git.rs:139-180` (`has_connections` / `git_driver`) reads
`connections_config::declared_for("git")` + `QFS_GIT_*` env only — zero `path_binding` references.
`describe.rs:70-72` documents both as the "declared-connection seam", but only sql was converged.
Mirror the sql pattern (`path_binding_sql_connections`) for git.

## Part B — `--` line comments in the connections parser

`crates/core/src/ddl/connections.rs::split_statements` (62-80) splits only on top-level `;`; no
comment stripping exists anywhere in `crates/parser/src`. A leading `-- comment` gets concatenated
into the next statement, which then silently fails to parse and drops a legitimate declaration
(best-effort skip at connections.rs:38) — worse than merely "unsupported". `deploy/dev/README.md:28-29`
documents the gap. Strip `--` line comments before/inside statement splitting; add tests.

## Part C — SQL NUMERIC/TIMESTAMP/UUID/JSON value round-trips

DDL/type mapping is already complete (`crates/sql-core/src/dialect.rs:102-186`), but live value
decode is not: `crates/qfs/src/sql_backends.rs::pg_value` (179-209) only decodes
BOOL/INT2/4/8/FLOAT4/8/BYTEA; NUMERIC/TIMESTAMP(TZ)/UUID/JSON fall through to
`try_get::<Option<String>>` and **error** on read. `my_value` (405-421) has an analogous lossy
fallback. The module doc admits it is a follow-up. Add explicit decode arms + hermetic/live
round-trip tests, matching the `dialect.rs` contract.

## Key files

- A: `crates/qfs/src/git.rs`, `crates/qfs/src/sql.rs` (pattern), `crates/qfs/src/describe.rs`.
- B: `crates/core/src/ddl/connections.rs`, `crates/parser/src`.
- C: `crates/qfs/src/sql_backends.rs`, `crates/sql-core/src/dialect.rs` (target contract).

## Considerations

- Concern part (b) "declared /sql is SQLite-only" is already RESOLVED (`sql.rs:216-258` builds
  sqlite/postgres/mysql declared handles) — dropped from scope.
- Source concern: `.workaholic/concerns/11-postgres-mysql-declarations-for-the-declared.md`.
