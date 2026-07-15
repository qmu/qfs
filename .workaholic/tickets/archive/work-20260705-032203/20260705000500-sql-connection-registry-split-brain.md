---
created_at: 2026-07-05T00:05:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, DB, Infrastructure]
effort: 4h
commit_hash: 5fbaeed
category: Changed
depends_on: []
---

# The `/sql` connection registries are split-brain: run and describe read different sources

Surfaced verifying the SQLite DBMS-management ticket (20260704001233) with the built binary. The
core create→table→insert→select→drop flow works, but **which connection mechanism you use decides
whether run *or* describe works — never both**, because the runtime driver registry and the describe
registry are populated from different, non-overlapping sources.

## Demonstrated (hermetic, built binary v0.0.20)

Three connection mechanisms, three different broken combinations:

| connection declared via | `qfs run` on `/sql/<conn>` | `qfs describe /sql/<conn>/<table>` |
| --- | --- | --- |
| `QFS_SQL_<conn>` env var (deprecated) | ✅ works | ❌ `unknown_mount … (describe registry)` |
| `connections.qfs` (`QFS_CONNECTIONS`) | ✅ works | ❌ `unknown_mount … (describe registry)` |
| `qfs connect …` (persisted DB binding) | ❌ `no driver registered for sql` | ❌ `unknown_mount … (describe registry)` |

So `describe /sql/<anything>` **never** works today, and the primary documented connect path
(`qfs connect /db --driver sqlite --at 'file:app.db'`, cli.md) does not wire the runtime `sql`
driver at all.

## Root causes (traced this session)

1. **Runtime side** (`crates/qfs/src/commit.rs:262`): the runtime `sql` apply/read driver is built
   only when `crate::sql::has_connections()` is true, and `has_connections()` reads env-var +
   `connections.qfs` (`connections_config.rs`) — it does **not** consult persisted `qfs connect` DB
   bindings. A `qfs connect` sqlite binding is listed by `connect --list` but is invisible to the
   runtime, so a subsequent `qfs run` reports `no driver registered for sql`.
2. **Describe side** (`crates/qfs/src/describe.rs:70`, `register_defined_paths`): the describe
   registry mounts a **cred-free driver per DB binding**, but `sql` has *no cred-free describe
   constructor* (`driver_type_mount` returns `None` for `sql`/`git`), so even a persisted binding
   can't be described. And env-var / `connections.qfs` connections are not DB bindings at all, so
   they never reach `register_defined_paths` in the first place.

The two registries need **one** connection source of truth: whatever mechanism declares a `/sql`
connection must feed both the runtime driver build and the describe mount, and `sql` needs a
describe-time catalog mount (built from the declared connection's introspected schema, the same
`ConnHandle` catalog the read path already builds) so `describe /sql/<conn>/<table>` reflects the
live/created columns.

## Quality gate

- With a connection declared by **any one** mechanism (pick the non-deprecated one — `qfs connect`
  and/or `connections.qfs`), all of: `qfs run` reads/writes, `qfs describe /sql/<conn>` (SHOW
  TABLES shape), and `qfs describe /sql/<conn>/<table>` (columns + verbs) work in the **same** fresh
  hermetic environment.
- After a `CREATE TABLE` commit, `describe /sql/<conn>/<table>` shows the new columns (the DBMS
  ticket's "DESCRIBE reflects the new catalog" gate, now actually exercised through the binary).
- Decide and document the ONE canonical local-connection mechanism; if `qfs connect` is it, env-var
  and `connections.qfs` either desugar into the same store or are honestly scoped as read-only shims
  in the docs (qfs is experimental — a hard break is fine).
- Hermetic e2e coverage for `describe /sql/<conn>/<table>` (there is none today — the DBMS ticket's
  cookbook ratchet only parse-checks recipes and never drives the one-shot describe/addressing path,
  which is exactly how the `REMOVE TABLE` one-shot addressing bug and this describe gap both shipped).

## DECISION (2026-07-05, owner) — canonical mechanism = `qfs connect` (persisted DB binding)

Surfaced at the overnight `/drive` and answered by the owner: the ONE canonical local-connection
mechanism is **`qfs connect`** (the persisted `path_binding` DB binding) — already "the SINGLE
SOURCE OF TRUTH" (`path_binding.rs:12`) and the primary documented path (cli.md). This ticket is now
**unblocked and implementable** (no further owner decision needed). Implement to this decision:

- **Converge both registries on `path_binding`.** The runtime `sql` driver build
  (`commit.rs:262`, `crate::sql::has_connections`) and the describe mount
  (`describe.rs::register_defined_paths`) must BOTH read the persisted `qfs connect` sqlite/pg/mysql
  bindings — not just env-var/`connections.qfs`. A `qfs connect` binding must wire the runtime `sql`
  driver (fixing `no driver registered for sql`).
- **Add a `sql` cred-free describe catalog mount** (`describe.rs`, the `driver_type_mount`/
  `cred_free_driver` gap for `sql`/`git`), built from the declared connection's introspected schema
  (the same `ConnHandle` catalog the read path builds, `driver-sql/src/conn.rs`) so
  `describe /sql/<conn>/<table>` reflects live/created columns.
- **`env-var` / `connections.qfs` become read-only shims** (or desugar into the same `path_binding`
  store) — honestly scoped in the docs. qfs is experimental: a hard break is fine.
- **Add hermetic e2e for `describe /sql/<conn>/<table>`** (none exists today — the gap that shipped
  the addressing/describe bugs).

### ⚠ ONE UNRESOLVED SUB-DECISION (found 2026-07-05 during the overnight attempt) — needs the owner

The "qfs connect canonical" decision does NOT pin the **path model** for the sql conn-name mapping,
and it is genuinely ambiguous:

- `conn_registry` (`sql.rs:211`) reads `connections_config::declared_for` (a `connections.qfs` FILE)
  + `QFS_SQL_*` env vars, and keys the sql conn by a bare NAME (`shop`, addressed `/sql/shop/<table>`).
- cli.md documents `qfs connect /db --driver sqlite --at 'file:app.db'` — a `path_binding` at the
  user path **`/db`**, NOT `/sql/db`. But the failing/target address in the bug table is `/sql/<conn>`.

So: when a `qfs connect /db --driver sqlite` binding wires the runtime sql driver, is the connection
addressed at **`/db/<table>`** (the binding path IS the sql mount — a path alias) or **`/sql/db/<table>`**
(the last binding segment becomes the `/sql` conn name)? The two imply different `conn_registry` +
describe-mount wiring. My recommendation (implementable): **connect AT the sql path** — `qfs connect
/sql/shop TO sqlite AT '...'` binds conn `shop`, and `conn_registry` reads `path_binding` rows whose
`path` is under `/sql/` (last segment = conn name) — the least-surprising, mirrors `/sql/<conn>` and
needs no new alias machinery. But cli.md's `/db` example implies the alias model, so this is the
owner's call before implementation. **Everything else above is ready to build once this is picked.**

## DONE (2026-07-05, work-20260705-032203) — owner picked `/sql/<conn>`; run + describe converge

The owner chose the **connect-AT-the-sql-path** model (`qfs connect /sql/shop TO sqlite …` → conn
`shop` at `/sql/shop/<table>`). Implemented to it:

- **`conn_registry` reads the canonical `path_binding` source** (`sql.rs`): a new
  `path_binding_sql_connections()` projects each FULL-connect binding whose path is under `/sql/` and
  whose driver is `sqlite`/`postgres`/`mysql` into `(conn, driver, at, secret)` (conn = the segment
  after `/sql/`), opened into a live `ConnHandle` and registered LAST (so a persisted binding wins a
  clash with the deprecated env-var/`connections.qfs` shims). `has_connections()` now counts it too,
  so the runtime sql apply + read drivers (`commit::live_registry` / `shell::run_engine_and_reads`,
  both built from `sql_driver()`) wire from a `qfs connect` binding — fixing `no driver registered for sql`.
- **The `/sql` describe mount** (`describe.rs`): `describe_registry` now registers the LIVE
  `sql_driver()` when a connection resolves, so `describe /sql/<conn>` (SHOW TABLES) and
  `describe /sql/<conn>/<table>` (columns) reflect the introspected catalog — the same
  `crate::sql::sql_driver()` the runtime builds from, so the two registries CONVERGE on one source.
- **Read-only shims:** `QFS_SQL_*` env vars + `connections.qfs` still resolve as fallback (a
  persisted `qfs connect` binding wins a name clash). cli.md updated to connect AT `/sql/<conn>`.
- **Hermetic e2e** (`describe.rs::qfs_connect_sql_binding_converges_run_and_describe`): seed a
  `CONNECT /sql/shop TO sqlite AT '<file>'` binding + a table, then `has_connections()` is true AND
  `describe /sql/shop/items` resolves through the sql mount + reflects the `[id, name]` columns (the
  DBMS ticket's "DESCRIBE reflects the new catalog" gate, now exercised through the binding).

## Key files

- `packages/qfs/crates/qfs/src/commit.rs` — runtime driver registry build (the `has_connections`
  gate)
- `packages/qfs/crates/qfs/src/describe.rs` — describe registry + `register_defined_paths`; `sql`
  cred-free mount gap
- `packages/qfs/crates/qfs/src/sql.rs` / `connections_config.rs` / `connection.rs` — the connection
  sources that must converge
- `packages/qfs/crates/driver-sql/src/conn.rs` — the `ConnHandle` catalog a describe mount reuses
