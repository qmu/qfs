---
created_at: 2026-07-05T02:20:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 2h
commit_hash: 446d108
category: Changed
depends_on: []
---

# Env-mutating qfs-crate tests race under parallel `cargo test`, intermittently failing CI

## Problem (now demonstrably blocking releases)

The `qfs`-binary-crate tests that mutate process-global env (`XDG_CONFIG_HOME`, `QFS_PASSPHRASE`, …)
race under parallel `cargo test --workspace` and intermittently fail CI. It flagged as a known
follow-up in the sqlite DBMS ticket ("pass fully serialized"), but it **failed the v0.0.21 CI run**
(PR #19) and blocked the ship — so it has graduated from "flaky note" to "release blocker".

Observed CI failure (PR #19, `build + test (native)`):

```
init::tests::init_is_idempotent_and_enforces_one_operator … FAILED
  first init succeeds: "opening the system database: sqlite: UNIQUE constraint failed: schema_version.version"
oauth::tests::* … FAILED   (cascade: the panic poisons the shared ENV_LOCK → .lock().unwrap() panics)
store::tests::{open_project_db…, open_system_db…, *xdg*} … FAILED
```

## Root cause (traced)

Two independent test-hygiene faults let two threads open the **same** DB concurrently:

1. **`remove_var("XDG_CONFIG_HOME")` falls back to the shared `$HOME/.config/qfs`.** Many `oauth.rs`
   tests `std::env::remove_var("XDG_CONFIG_HOME")` (lines ~1059–1599) so the opener resolves to
   `$HOME/.config/qfs/system.db` — a **single shared file** on the CI runner. A fresh-per-test tempdir
   (as `init.rs`/`account.rs`/`commit.rs`/`shell.rs` do via `with_fresh_home`) would isolate them.
2. **Inconsistent `ENV_LOCK` coverage.** The crate-wide `crate::ENV_LOCK` (`lib.rs:74`) is held by
   `init`/`account`/`commit`/`store`/`shell` and by ~3 `oauth` tests, but the *other* `oauth` tests
   only `remove_var` without acquiring it — so they run concurrently with the locked tests and each
   other. `connections_config.rs` and `secret_ref.rs` also `set_var` without the lock (they touch only
   namespaced `QFS_SQL_*`/`QFS_SECRET_REF_TEST_*` vars, so lower risk, but still unlocked).

When two migrates open the same `system.db` concurrently, both pass the `SELECT … FROM schema_version`
"not applied" check, then both `INSERT INTO schema_version` → the second gets `UNIQUE constraint
failed: schema_version.version`. The first-panicking test poisons `ENV_LOCK`, cascading `.lock().unwrap()`
panics across every sibling env test. (The migration runner is correct for its single-process
start-time contract; the fault is the tests opening a shared DB concurrently, not the runner.)

## Progress

- **Runner defense-in-depth: DONE** on branch `work-20260705-015204` (v0.0.21) — the migration
  apply now uses an `IMMEDIATE` (write-locking) transaction that re-checks the version under the
  lock, plus a `busy_timeout` in `Db::open`, so concurrent opens of the same DB serialize instead of
  racing the `schema_version` check-then-insert. A new `concurrent_migrations_on_the_same_db_do_not_race`
  test proves 8 threads migrating one file DB no longer hit `UNIQUE constraint`. This deterministically
  killed the CI crash (the previously-flaky `init`/`oauth`/`store` env tests now pass parallel, 2015
  green). **The primary test-hygiene fix below remains** — the env tests still bleed `XDG_CONFIG_HOME`
  under some interleavings, they just no longer crash.

## Fix (pick the test-hygiene fix; the runner change is optional defense-in-depth)

- **Primary (test hygiene):** every env-mutating test acquires the crate-wide `crate::ENV_LOCK` (a
  single shared lock) and sets `XDG_CONFIG_HOME` to a **fresh per-test tempdir** — never `remove_var`
  it to fall back to the shared `$HOME`. Route the `oauth.rs` tests through a `with_fresh_home`-style
  helper like the other modules. Bring `connections_config.rs` / `secret_ref.rs` under the lock too.
  Consider a `#[cfg(test)]` guard that panics if a test opens the store with `XDG_CONFIG_HOME` unset,
  so a future test can't silently reintroduce the shared-`$HOME` fallback.
- **Optional (runner defense-in-depth):** make the migration check-and-apply atomic under concurrency
  — wrap the per-migration `SELECT`-then-`INSERT` in an `IMMEDIATE` (write-locking) transaction so a
  second concurrent open blocks, then sees the version applied and skips. This hardens the
  start-time runner without masking the test bug, but is not required if the tests are isolated.
- Avoid making `ENV_LOCK` a poison trap: a helper that clears the poison (or a non-poisoning lock)
  keeps one failure from cascading into a wall of unrelated panics.

## Quality Gate

- `cargo test --workspace` passes **reliably under the default parallel runner** across repeated runs
  (not just `--test-threads=1`); CI `build + test` is green deterministically.
- No test resolves the store to the shared `$HOME/.config/qfs` — every env-mutating test uses a fresh
  tempdir under the shared lock.
- `clippy`/`fmt`/`gen-docs`/`gen-skills` green. Bump the patch before the PR.

## Key files

- `packages/qfs/crates/qfs/src/oauth.rs` (the `remove_var` tests → fresh tempdir + `ENV_LOCK`)
- `packages/qfs/crates/qfs/src/{init,account,commit,store,shell}.rs` (the `with_fresh_home` pattern to mirror)
- `packages/qfs/crates/qfs/src/{connections_config,secret_ref}.rs` (unlocked `set_var` tests)
- `packages/qfs/crates/qfs/src/lib.rs` (`ENV_LOCK`; consider a non-poisoning variant)
- `packages/qfs/crates/store/src/migrate.rs` (optional IMMEDIATE-transaction hardening)
