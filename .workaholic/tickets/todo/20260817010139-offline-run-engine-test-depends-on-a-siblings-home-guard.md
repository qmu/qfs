---
created_at: 2026-08-17T01:01:39+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
---

# `offline_run_engine_does_not_mount_server` passes only while a sibling test's `HomeGuard` happens to be live

## Overview

`crates/qfs/src/provision.rs`'s `offline_run_engine_does_not_mount_server` calls
`crate::shell::run_engine_and_reads()` without opening a `testenv::HomeGuard`. That call resolves
the active safety mode, which opens the System DB, which resolves its path through
`store::default_system_db_path()` — and in a `cfg(test)` build that function **panics** when
`XDG_CONFIG_HOME` is unset:

```
thread 'provision::tests::offline_run_engine_does_not_mount_server' panicked at crates/qfs/src/store.rs:54:5:
the store resolved to the shared $HOME/.config/qfs (XDG_CONFIG_HOME unset) inside a qfs unit test
  — wrap the test in `testenv::HomeGuard` so it uses an isolated config home
```

`HomeGuard` sets `XDG_CONFIG_HOME` process-wide and restores it on drop, so whether this test sees
the variable set is decided by which *other* test is running concurrently. It therefore passes on a
busy scheduler and fails on a quiet one — the guard at `store.rs:54` exists precisely to make that
class of accident loud, and here it is firing on a test that never opted into isolation.

Measured 2026-08-17 in the routine's container, on **unmodified `origin/main` at `cd8be38`**:

```
$ cargo test -p qfs --lib offline_run_engine_does_not_mount_server
test result: FAILED. 0 passed; 1 failed          # alone: deterministic

$ cargo test -p qfs --lib
test result: FAILED. 484 passed; 1 failed        # in the full lib suite, same test
```

GitHub Actions is green on that same commit (`ci.yml`, run 31981433022), which is the tell: nothing
about the change under test differs, only the interleaving.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — a test's outcome must not depend
  on ambient process state another test happens to be holding.
- `workaholic:implementation` / `policies/directory-structure.md`.
- `workaholic:operation` / `observability` — a suite that is green by scheduling luck reports
  something other than the system's state.

## Key Files

- `packages/qfs/crates/qfs/src/provision.rs` — `offline_run_engine_does_not_mount_server`, the test
  that calls `run_engine_and_reads()` bare. Its siblings in the same module (e.g.
  `destroy_requires_the_irreversible_ack`) open `HomeGuard` first; this one does not.
- `packages/qfs/crates/qfs/src/shell.rs` — `run_engine_and_reads()` resolves the active safety mode
  before building the engine, which is what reaches the store at all.
- `packages/qfs/crates/qfs/src/store.rs` — `default_system_db_path()` and
  `forbid_shared_home_fallback_in_tests()` (the `cfg(test)` guard, ticket 20260705022000).
- `packages/qfs/crates/qfs/src/testenv.rs` — `HomeGuard`, the sanctioned isolation, and its
  crate-wide `ENV_LOCK`.

## Implementation Steps

1. Reproduce: run the test alone (`cargo test -p qfs --lib offline_run_engine_does_not_mount_server`)
   and confirm the `store.rs:54` panic with `XDG_CONFIG_HOME` unset.
2. Sweep the crate for the same shape — every `#[test]` reaching `run_engine_and_reads`,
   `open_system_db`, or any other store opener without a `HomeGuard` in scope. Fix the class, not
   only the one that happened to be caught.
3. Give each such test its isolated config home (`let _home = HomeGuard::new();`), which also takes
   `ENV_LOCK` and so serialises it against the tests whose env it was silently borrowing.
4. Consider whether the guard should also fire when `XDG_CONFIG_HOME` *is* set but was set by
   another test's guard — i.e. whether a test can be made to prove its own isolation rather than
   inherit one. Record the finding either way; do not widen the guard speculatively.

## Quality Gate

**Acceptance criteria**

- `cargo test -p qfs --lib offline_run_engine_does_not_mount_server` passes when run alone.
- `cargo test -p qfs --lib -- --test-threads=1` passes, which is the ordering that removes every
  accidental overlap.
- No `#[test]` in the `qfs` crate reaches a store opener without a `HomeGuard` in scope.

**Verification method**

- The two `cargo test` invocations above, plus the sweep from step 2 recorded in the Final Report.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` all exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-17 while driving `20260817001110`. It is
  not a consequence of that ticket's change: the identical failure reproduces on an unmodified
  `main` checkout in the same container, so the branch's own gate was read against that baseline.
- The fix is deliberately not "set `XDG_CONFIG_HOME` in CI": that would hide the same class of
  accident everywhere instead of isolating the test that needs isolating
  (`packages/qfs/crates/qfs/src/store.rs` lines 43-56 argues exactly this).
