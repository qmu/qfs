---
created_at: 2026-08-17T01:01:39+00:00
status: done
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260818-204014
---

# `offline_run_engine_does_not_mount_server` is the tenth test with no config home of its own, and the sweep missed it

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

This is exactly the class `20260816205752-nine-qfs-unit-tests-depend-on-an-ambient-xdg-config-home.md`
closed for nine tests one day earlier (PR #61, commit `66b57f4`, "cargo test --workspace is green
with `XDG_CONFIG_HOME` unset as well as set"). That claim is not true yet: this test predates the
sweep (it has been in `provision.rs` since `f9387de`) and the sweep did not reach it.

Measured 2026-08-17 in the routine's container, on **unmodified `origin/main` at `cd8be38`**:

```
$ echo "${XDG_CONFIG_HOME:-<unset>}"
<unset>
$ cargo test -p qfs --lib offline_run_engine_does_not_mount_server
test result: FAILED. 0 passed; 1 failed

$ cargo test -p qfs --lib
test result: FAILED. 484 passed; 1 failed        # the full lib suite, same single failure

$ XDG_CONFIG_HOME=/tmp/x cargo test -p qfs --lib offline_run_engine_does_not_mount_server
test result: ok. 1 passed; 0 failed
```

So the result is decided by the ambient environment, not by the code under test. GitHub Actions is
green on that same commit (`ci.yml`, run 31981433022), which is what has kept it invisible.
`HomeGuard` also sets `XDG_CONFIG_HOME` process-wide for its lifetime, so a concurrently-running
guarded sibling can mask the failure as well.

## Related History

The previous sweep of this exact class fixed nine tests and stopped short of the tenth; its own
insight — "the reliable question is not *does this test touch the environment* but *can anything
under it resolve a store path*" — is the one that would have caught this call chain.

- [20260816205752-nine-qfs-unit-tests-depend-on-an-ambient-xdg-config-home.md](.workaholic/tickets/archive/work-20260816-210358/20260816205752-nine-qfs-unit-tests-depend-on-an-ambient-xdg-config-home.md) — the same defect, nine other tests (same `store.rs:54` guard)

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

1. Reproduce with `XDG_CONFIG_HOME` unset (`env -u XDG_CONFIG_HOME cargo test -p qfs --lib
   offline_run_engine_does_not_mount_server`) and confirm the `store.rs:54` panic.
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

- With `XDG_CONFIG_HOME` **unset**: `cargo test -p qfs --lib offline_run_engine_does_not_mount_server`
  passes when run alone.
- With `XDG_CONFIG_HOME` **unset**: `cargo test --workspace` is green, and stays green under
  `-- --test-threads=1` (the ordering that removes every accidental overlap).
- No `#[test]` in the `qfs` crate reaches a store opener without a `HomeGuard` in scope.

**Verification method**

- The `cargo test` invocations above run with `XDG_CONFIG_HOME` explicitly unset (`env -u
  XDG_CONFIG_HOME …`) — the variable being set is what hides this defect — plus the sweep from
  step 2 recorded in the Final Report.

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

## Final Report

Development completed as planned, and the sweep (step 2) turned out to be the substance of the
ticket rather than a formality: the class is **fourteen** tests, not one.

### What was measured

Reproduction, in the worktree with `XDG_CONFIG_HOME` unset:

```
$ env -u XDG_CONFIG_HOME cargo test -p qfs --lib offline_run_engine_does_not_mount_server
thread '...offline_run_engine_does_not_mount_server' panicked at crates/qfs/src/store.rs:54:5:
  the store resolved to the shared $HOME/.config/qfs (XDG_CONFIG_HOME unset) inside a qfs unit test
test result: FAILED. 0 passed; 1 failed
```

The stack confirms the ticket's chain exactly: `run_engine_and_reads` → `resolve_active_safety_mode`
→ `SystemDbBackend::open_default` → `open_system_db` → `default_system_db_path`.

The sweep was run **empirically rather than by grep**, because the `cfg(test)` panic already *is* a
perfect detector of "reaches a store opener without a `HomeGuard`" — it just has to be given an
environment where nothing masks it. Running the whole lib suite with `XDG_CONFIG_HOME` unset **and**
`--test-threads=1` (the ordering that removes every accidental overlap) enumerated the class:

```
$ env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1
test result: FAILED. 505 passed; 14 failed; 1 ignored
```

The fourteen, by module:

- `agent.rs` — `agent_run_previews_without_commit`, `agent_run_commits_an_in_grant_function`,
  `agent_run_denies_an_ungranted_function`, `agent_run_no_policy_is_default_denied` (all reach the
  store through `run_agent`)
- `job.rs` — `job_run_commits_a_defined_plan_once`, `job_run_irreversible_without_ack_is_blocked`,
  `job_run_previews_without_commit`, `job_run_policy_denied_aborts` (through `run_job`)
- `describe.rs` — `mail_drafts_describes_cred_free`, `all_registered_mounts_describe_cred_free`
  (through `describe_registry()`)
- `commit.rs` — `a_gdrive_named_mount_registers_a_lazy_apply_driver_under_the_outer_id`,
  `a_drive_kind_mount_is_accepted_as_an_alias_for_gdrive` (through `register_cloud_mounts`)
- `shell.rs` — `repl_commit_targets_the_session_root_not_the_filesystem_root` (through
  `run_repl_with_history_and_apply`'s apply hook)
- `provision.rs` — `offline_run_engine_does_not_mount_server`, the one the ticket named

Each got `let _home = crate::testenv::HomeGuard::new();` as its first statement — the sanctioned
isolation, which also takes `ENV_LOCK` and so serialises it against the siblings whose env it was
borrowing. Nothing else changed; no production code was touched.

### What is green now

```
$ env -u XDG_CONFIG_HOME cargo test -p qfs --lib offline_run_engine_does_not_mount_server
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 519 filtered out

$ env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1
test result: ok. 519 passed; 0 failed; 1 ignored

$ env -u XDG_CONFIG_HOME cargo test --workspace      # TEST_EXIT=0, 149 green result lines
$ cargo clippy --workspace --all-targets -- -D warnings   # exit 0
$ cargo fmt --all --check                                  # exit 0
$ cargo run -p xtask -- gen-docs --check                   # docs are in sync
$ cargo run -p xtask -- gen-skills --check                 # skills are in sync
```

### Step 4 — should the guard fire when `XDG_CONFIG_HOME` is set by *another* test's guard?

**Finding recorded; the guard is deliberately not widened**, per the ticket's own instruction.

The evidence for the concern is now direct rather than theoretical: thirteen of the fourteen were
invisible under the default parallel harness and appeared only once the run was serialised. The
masking mechanism is exactly the one the ticket names — `HomeGuard` sets `XDG_CONFIG_HOME`
process-wide for its lifetime, so any unguarded test scheduled alongside a guarded one silently
inherits that sibling's isolated home and passes.

A guard that fired unless *the calling test itself* holds a `HomeGuard` would need thread-local
ownership (a marker set in `HomeGuard::build` and cleared in `Drop`), because the crate-wide
`ENV_LOCK` makes at most one guard live at a time and a process-global counter therefore cannot
distinguish "I am isolated" from "somebody else is". That design was rejected here for a concrete
reason, not a stylistic one: several tests in this crate reach store openers from threads they did
not spawn themselves (the `provision.rs` daemon fixtures are the clearest case), so a
thread-locality requirement would convert real passes into false panics, and diagnosing those is
strictly harder than the defect it would catch.

The cheap detector that *does* work needs no code at all and is recorded here so the next sweep can
reuse it: **run the crate's lib suite with `XDG_CONFIG_HOME` unset and `--test-threads=1`**. It is
deterministic, it enumerated the whole class in one 67-second pass, and it needs nothing from the
guard beyond what `store.rs:54` already does.

### Discovered Insights

- **Insight**: the `store.rs:54` `cfg(test)` panic is already a complete detector for this defect
  class; what was missing was an environment in which it could fire. Two ambient conditions each
  suppress it independently — `XDG_CONFIG_HOME` being set at all, and a guarded sibling running
  concurrently — and the previous sweep's nine-test result is what a run under *one* of those
  suppressors looks like.
  **Context**: any future "does this test isolate its config home?" question should be answered by
  `env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1`, never by reading call
  graphs. The call-chain reading is what missed the tenth test last time, and it would have missed
  the other four modules this time: `describe_registry()` and `register_cloud_mounts` do not look
  like store openers from their names.
- **Insight**: `--test-threads=1` is load-bearing for this check, not a tidiness preference. Under
  the default parallel harness this same tree reported one failure; serialised, it reported
  fourteen. A suite that is green only under parallel scheduling is green by luck.
  **Context**: the ticket's acceptance criterion already said "stays green under
  `-- --test-threads=1`" — that clause is the whole test, and reading it as a redundant restatement
  of the workspace run would have shipped thirteen live instances of the defect.
