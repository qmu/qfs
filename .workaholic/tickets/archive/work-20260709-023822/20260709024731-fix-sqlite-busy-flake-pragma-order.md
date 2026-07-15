---
created_at: 2026-07-09T02:47:31+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort:
commit_hash: 32a87da
category: Changed
depends_on:
mission:
---

# Fix the SQLite "database is locked" flaky class: arm busy_timeout before the WAL pragma

## Overview

Single tests intermittently panic with `Sqlite("database is locked")` under parallel
`cargo test --workspace` load — in CI and locally. Confirmed instances (any of them can be the one
that trips on a given run):

- `migrate::concurrency_tests::concurrent_migrations_on_the_same_db_do_not_race`
  (`qfs-store`, `crates/store/src/migrate.rs`)
- `provision::tests::destroy_requires_the_irreversible_ack` (`qfs`, `crates/qfs/src/provision.rs`)
- `cf::tests::cf_account_secret_resolves_from_the_qfs_vault` (`qfs`, `crates/qfs/src/cf.rs:366`,
  panic `opening the system database: sqlite: database is locked`)

**Root cause (verified in source):** `qfs_store::Db::open`
(`crates/store/src/lib.rs:78-94`) — the single embedded-SQLite open seam every scope
(SystemDb/ProjectDb) and every caller flows through — sets its pragmas in the wrong order. It runs
`PRAGMA journal_mode=WAL` **first** and installs `busy_timeout(5s)` **last**. Switching a connection
to WAL takes a brief write/exclusive lock and is the connection's *first locking statement*; because
no busy handler is armed yet, any concurrent connection that momentarily holds a lock on the same
file makes the WAL switch return SQLITE_BUSY **immediately** instead of waiting out the 5-second
timeout. rusqlite renders that as `database is locked`, `StoreError::Sqlite` wraps it
(`lib.rs:535-542` stringifies, losing the `DatabaseBusy` code), and call sites prefix
`opening the system database:` (`crates/qfs/src/identity.rs:44`, `provision.rs:635`).

Contention arises without any cross-test file sharing (the prior ticket's `HomeGuard` already gives
each env-mutating test a fresh `XDG_CONFIG_HOME` tempdir): the 8-thread migrate concurrency test
races 8 fresh `Db::open` calls on one tempfile; the `cf`/`provision` flows overlap connections to one
file *within a single test* (e.g. `commit::networked_credential` holds a live `SqliteSecrets`
project.db connection while `cloud_bind_allowed → operator_signed_in()` opens system.db and
`consent_recorded()` opens another project.db connection — `crates/qfs/src/commit.rs`). Under
parallel `cargo test` CPU/IO pressure those windows are hit intermittently.

**Fix:** arm `busy_timeout` **first** — immediately after `source.connect()`, before
`journal_mode=WAL` and `foreign_keys=ON` — so every locking statement on the connection, including
the WAL switch itself, waits under contention instead of failing instantly.

**Same-class unprotected path (in scope, per owner):** `crates/qfs/src/sql.rs:45` opens user `/sql`
SQLite DBs via a raw `rusqlite::Connection::open` with **no** `busy_timeout` at all. Give it the
same busy-handler protection. Note the asymmetry: `busy_timeout` is per-connection (safe anywhere),
but `journal_mode=WAL` is a **persistent property of the file** — correct for qfs's own
system/project DBs, *not* something qfs should impose on a user's SQLite file. So `sql.rs` gets
`busy_timeout` only, never a WAL rewrite of the user's journal mode.

## Policies

- `workaholic:implementation` / `policies/test.md` — per-test DB isolation stays (each test its own
  records/tempdir); race conditions are exactly the boundary "to pick up and leave behind as tests" —
  the fix ships with a stress reproducer that stays in the tree.
- `workaholic:implementation` / `policies/domain-layer-separation.md` — connection configuration
  (busy_timeout/WAL/pragma order) is the vendor boundary's job: fix once in `Db::open` (qfs-store)
  and the one raw open in `sql.rs`; no rusqlite type leaks into domain signatures.
- `workaholic:operation` / `policies/ci-cd.md` — a flaky red destroys the "green means healthy"
  evidence chain; the same `cargo test --workspace` must be deterministic locally and in CI.
- `workaholic:implementation` / `policies/coding-standards.md` — clippy-clean under
  `--workspace --all-targets -- -D warnings` (never `--all-features`); fmt clean.

## Key Files

Verified anchors at `main` post-v0.0.36 (2026-07-09):

- `packages/qfs/crates/store/src/lib.rs:78-94` — **primary fix**: `Db::open` reorders to
  `busy_timeout(5s)` → `journal_mode=WAL` → `foreign_keys=ON`. Keep the doc comment honest about WHY
  the order matters (the WAL switch is the first locking op and must already have the busy handler).
- `packages/qfs/crates/store/src/lib.rs:535-542` — `StoreError::Sqlite(String)` / the
  `From<rusqlite::Error>` seam. No retry layer needed once the busy handler is armed; do not widen
  the error taxonomy in this ticket.
- `packages/qfs/crates/store/src/migrate.rs:273-301` — the 8-thread
  `concurrent_migrations_on_the_same_db_do_not_race` reproducer; harden into the stress gate
  (more threads × per-thread open+migrate iterations, bounded runtime) so the OLD ordering fails it
  reliably and the new ordering holds.
- `packages/qfs/crates/qfs/src/sql.rs:45` — the raw `Connection::open` for user `/sql` DBs: add
  `busy_timeout` (NO WAL — a user file's journal mode is not qfs's to change).
- `packages/qfs/crates/qfs/src/commit.rs` / `cf.rs:359-383` / `provision.rs:835-871` — the
  overlapping-open flows the flake surfaced in; no behavior change expected here, they are the
  regression evidence (their tests must stay green across repeated runs).
- `packages/qfs/crates/qfs/src/testenv.rs` — `HomeGuard`/`ENV_LOCK` isolation stays as-is (it fixed
  the *cross-test* sharing; this ticket fixes the *intra-flow/parallel-load* residual).

## Related History

- [20260705022000-serialize-env-mutating-tests-migration-race.md](.workaholic/tickets/archive/work-20260705-032203/20260705022000-serialize-env-mutating-tests-migration-race.md)
  — the shipped ancestor (v0.0.21, commit `bff500d`): added the IMMEDIATE migration transaction,
  the 5s `busy_timeout`, WAL, and `HomeGuard`. It closed the `schema_version` UNIQUE race but left
  the pragma-ordering gap this ticket closes.
- [20260704001233-implement-sqlite-dbms-management.md](.workaholic/tickets/archive/work-20260704-181053/20260704001233-implement-sqlite-dbms-management.md)
  — origin of `Db::open` and the migration runner.
- Operating-stance note: the `.claude` memory `flaky-migration-concurrency-test` records
  "rerun the failed job, don't debug" for this failure class. **This ticket retires that stance** —
  once shipped, a `database is locked` failure is a real regression again, not a flake to rerun.

## Implementation Steps

1. **Reorder `Db::open` pragmas** (`store/src/lib.rs`): `busy_timeout` immediately after
   `connect()`, then `journal_mode=WAL`, then `foreign_keys=ON`; update the comment to state the
   ordering invariant (busy handler armed before the first locking statement).
2. **Protect the user-DB open** (`qfs/src/sql.rs:45`): add the same `busy_timeout` to the raw
   `Connection::open`; explicitly no WAL on user files (comment why).
3. **Stress reproducer as the gate** (`store/src/migrate.rs`): harden the concurrency test —
   raise to ~16 threads with a small per-thread loop of open+migrate iterations on one shared
   tempfile (bounded seconds, not minutes). **Pre-fix evidence:** run this hardened test against the
   old ordering (stash the reorder) and record that it trips `database is locked`; **post-fix gate:**
   the same test passes repeatedly.
4. **Overlapping-open regression test** (`store/src/lib.rs` tests): one connection held open on a
   file DB while a second `Db::open` on the same file runs its full pragma sequence — asserts the
   second open succeeds (waits) rather than failing busy.
5. **Repeat-run verification:** `cargo test -p qfs-store` and the two binary-crate test names
   (`provision::tests::destroy_requires_the_irreversible_ack`,
   `cf::tests::cf_account_secret_resolves_from_the_qfs_vault`) looped ~20× locally with default
   parallelism — zero `database is locked`.
6. **Version + docs:** bump the qfs patch (per shipped PR). No taught CLI surface changes → no
   plugin re-version, and gen-docs/gen-skills should be no-ops (`--check` still run).

## Quality Gate

**Acceptance criteria:**

- `Db::open` arms `busy_timeout` before any locking statement (order asserted by reading the code;
  the WAL switch can no longer fail instantly-busy).
- The hardened stress test (~16 threads × open+migrate loop on one shared file) **fails against the
  old pragma order** (recorded in the ticket/commit evidence) and **passes with the fix** —
  repeatedly, not once.
- The overlapping-open regression test passes: a second `Db::open` on a file already held open
  succeeds by waiting, never `database is locked`.
- `sql.rs` user-DB opens carry `busy_timeout`; a user file's `journal_mode` is untouched (asserted:
  opening a user DB does not create `-wal`/`-shm` side files on a rollback-journal DB).
- 20 consecutive local runs of `cargo test -p qfs-store` plus the two named binary-crate tests with
  default parallelism: zero `database is locked` panics.
- Full gates green: `cargo test --workspace`, clippy `-D warnings` (not `--all-features`),
  `cargo fmt --all --check` (never piped), `gen-docs --check`, `gen-skills --check`,
  `check-migrations` (no shipped migration body edited — the pragma change is in `Db::open`, not a
  migration).

**Verification method:**

- Stress + regression tests in-tree (`cargo test -p qfs-store`); the pre-fix failure demonstrated by
  temporarily reverting the reorder (evidence noted in the commit body, revert not committed).
- Looped local runs (step 5) as the statistical check; CI observation is a trailing signal, not the
  gate.

**Gate:** hardened stress test red-on-old-order / green-on-fix, overlapping-open test green, 20×
looped runs clean, all workspace gates green.

## Considerations

- **This is the root-cause fix for the class the "rerun, don't debug" memory covered** — after ship,
  update/retire that memory note; a recurrence is a regression to investigate.
- `StoreError::Sqlite(String)` loses the `ErrorCode::DatabaseBusy` classification. With the busy
  handler armed there is no retry layer to build, so leave the taxonomy alone (out of scope).
- `busy_timeout` is per-connection and safe everywhere; **WAL is a persistent file property** —
  correct for qfs-owned system/project DBs, never imposed on user `/sql` files.
- Bounded stress: keep the hardened test within a few seconds (threads × small iteration count), so
  the suite stays fast; the 20× loop is a local verification step, not an in-tree cost.
- Experimental stance: no compat shims, no risk framing — a straight reorder + protection sweep.
- Commit via `workaholic:commit` `commit.sh` with explicit file args; never `git add -A`
  (shared-tree concurrent sessions).
