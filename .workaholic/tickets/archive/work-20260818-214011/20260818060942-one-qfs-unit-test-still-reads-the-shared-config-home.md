---
created_at: 2026-08-18T06:09:42+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260818-214011
---

# One qfs unit test still reads the shared config home, so the suite result is a race

## Overview

Observed while driving `20260816161143`/`161144`/`161145` on branch `work-20260816-154228`
(2026-08-18). `cargo test --workspace` failed with one test red:

```
---- provision::tests::offline_run_engine_does_not_mount_server stdout ----
thread '…' panicked at crates/qfs/src/store.rs:54:5:
the store resolved to the shared $HOME/.config/qfs (XDG_CONFIG_HOME unset) inside a qfs unit
test — wrap the test in `testenv::HomeGuard` so it uses an isolated config home
```

**It is not this branch's defect.** The test body is byte-identical on `origin/main`, and running
that single test alone in a main-based worktree reproduces the same panic:

```sh
env -u XDG_CONFIG_HOME cargo test -q -p qfs --lib \
  provision::tests::offline_run_engine_does_not_mount_server
# test result: FAILED. 0 passed; 1 failed
```

This is the tenth instance of the class that story `work-20260816-210358` closed — nine qfs unit
tests that inherited their config-home isolation from the ambient environment. That change added
the `forbid_shared_home_fallback_in_tests` guard in `crates/qfs/src/store.rs` and fixed the nine
tests it caught; this one was missed because it does not *look* like a store test. It reaches the
store indirectly, through `shell::run_engine_and_reads` → `sys::resolve_active_safety_mode` →
`SystemDbBackend::open_default`.

**Why CI is green anyway, and why that is the actual problem.** `HomeGuard` sets a
process-global `XDG_CONFIG_HOME`, and the test binary runs its tests in parallel threads. So this
test passes whenever some *other* test's guard happens to be alive at the moment it reads the
variable, and fails when it is not. CI's `build + test (native)` job was green on the same tree
(run `32104653107`, 2026-08-18). The suite is therefore not deciding anything about this test — it
reports whichever way the schedule fell — and the guard that exists to make config-home leakage
loud is being silenced by luck.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/testing.md` — what a green suite is allowed to mean

## Key Files

- `packages/qfs/crates/qfs/src/provision.rs` — `offline_run_engine_does_not_mount_server`, the
  test with no `HomeGuard`, around the block where every neighbouring test has one.
- `packages/qfs/crates/qfs/src/store.rs` — `forbid_shared_home_fallback_in_tests` and
  `default_system_db_path`: the guard and the fallback it refuses.
- `packages/qfs/crates/qfs/src/testenv.rs` — `HomeGuard` and how it scopes `XDG_CONFIG_HOME`.
- `packages/qfs/crates/qfs/src/shell.rs` — `run_engine_and_reads`, the entry point that reaches
  the system DB; whether it should is the real question in step 2.

## Related History

`.workaholic/stories/work-20260816-210358.md` — "Nine qfs unit tests inherited their config-home
isolation from the ambient environment and failed wherever `XDG_CONFIG_HOME` was unset; they now
carry their own, so the suite result no longer depends on the shell." That claim is one test short
of true.

## Implementation Steps

1. Reproduce deterministically: `env -u XDG_CONFIG_HOME cargo test -p qfs --lib
   provision::tests::offline_run_engine_does_not_mount_server` — running the one test alone removes
   the other guards and the race with them.
2. Decide which of two things is wrong, and record it. Either the test needs its own `HomeGuard`
   like its nine neighbours, **or** `run_engine_and_reads` should not be opening the system DB at
   all for an offline plan-building check — the test's own comment says the offline engine "never
   mounts /server", so touching the store to resolve a safety mode may be the surprise worth
   fixing instead of hiding.
3. Implement the choice.
4. Sweep for the rest of the class rather than fixing only the instance that surfaced: find every
   `#[test]` in `crates/qfs` that can reach `default_system_db_path` without holding a guard. A
   grep for `HomeGuard` finds the tests that have one, not the tests that need one.
5. Consider making the guard's own coverage non-racy — e.g. by having `HomeGuard` scope the
   variable per-test rather than per-process, or by running the affected tests single-threaded —
   so a missing guard fails every run instead of some runs.

## Quality Gate

**Acceptance criteria**

- `env -u XDG_CONFIG_HOME cargo test -p qfs --lib
  provision::tests::offline_run_engine_does_not_mount_server` passes on its own.
- No test in `crates/qfs` reaches `default_system_db_path`'s shared-home fallback, whatever the
  ambient `XDG_CONFIG_HOME` and whatever order the suite runs in.
- `cargo test --workspace` passes with `XDG_CONFIG_HOME` unset, repeatedly.

**Verification method**

- Run the single test alone, unset variable, before and after.
- Run `env -u XDG_CONFIG_HOME cargo test --workspace` several times; every run green.
- If step 5 is taken, deliberately remove one guard and confirm the suite goes red every time
  rather than sometimes — a guard never proven to fail is not a guard.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` green.

## Considerations

- Do not "fix" this by setting `XDG_CONFIG_HOME` in CI or in a test harness wrapper. That would
  make the suite green by giving every test a shared isolated home, which is the same class of
  cross-test coupling in a new coat, and the guard would stop catching anything.
- The second option in step 2 is a behaviour change in `shell.rs`, not a test change, and it
  deserves its own reasoning: `resolve_active_safety_mode` reading the store may be entirely
  correct and merely inconvenient to test.

## Final Report

Development completed as planned, with steps 1–4 found already landed and the work therefore
concentrated on step 5 — making the guard's own coverage non-racy.

**Steps 1–3 were closed before this drive reached the ticket.** `offline_run_engine_does_not_mount_server`
already carries `let _home = HomeGuard::new();` on `main`, added in `6ca1924` ("Isolate the config
home of 14 qfs unit tests", 2026-08-18). That commit also answered step 2's fork the way this
ticket's first option reads — the test gets a guard; `shell::run_engine_and_reads` keeps resolving
the active safety mode through the store — and swept the class (step 4) by running the lib suite
serialised with `XDG_CONFIG_HOME` unset, which found thirteen further unguarded tests the parallel
run had masked. Re-verified here before touching anything:
`env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1` → 521 passed, 0 failed.

**What was still open is the ticket's own last acceptance criterion**: "no test reaches the
shared-home fallback … whatever the ambient `XDG_CONFIG_HOME` and whatever order the suite runs
in". Nothing held that. The detector existed only as a command a person had to remember, and CI ran
the one shape of the suite in which the panic cannot fire.

**The experiment that sized it.** One guard was removed from
`offline_run_engine_does_not_mount_server` and the two shapes run against the identical tree:

| Run | Result |
| --- | --- |
| `env -u XDG_CONFIG_HOME cargo test -p qfs --lib` (the parallel harness, what CI ran) | **green**, 3 times out of 3 |
| `env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1` | **red** every time, on that one test |

So CI's `cargo test --workspace` is structurally unable to catch this class, and the serialised
run catches it deterministically. The guard was then restored and the tree confirmed byte-identical
(`git status` clean) before the real change.

**The change**: CI's `build-test` job gains that serialised run as a second step, and the three
places a developer or an agent looks for the verification surface now name it — `CLAUDE.md`'s Build
& test block (with a paragraph on why the plain suite cannot substitute), `docs/guide/repository.md`'s
gate table, and `docs/documentation-map.md`'s Gates row. `store.rs`'s guard doc comment carries the
reproducing command next to the panic it explains. No production code and no test body changed.

**The alternative that was rejected, deliberately.** Making the guard itself thread-aware — panic
when the live `HomeGuard` belongs to a different thread — would remove the concurrency suppressor
without a serialised run, but `6ca1924`'s own concerns record why it must not: tests in this crate
reach store openers from threads they did not spawn, so thread locality produces false panics. The
detector is therefore an *environment* the suite is run in, not a stricter assertion inside it.

### Discovered Insights

- **Insight**: A `cfg(test)` panic is only as strong as the weakest environment the suite is run
  in. This one has two independent suppressors — an ambient `XDG_CONFIG_HOME`, and `HomeGuard`
  setting that variable process-wide so a guarded sibling covers an unguarded test — and each on
  its own turns the guard into a coin flip.
  **Context**: The nine-test sweep of 2026-08-16 and the fourteen-test sweep of 2026-08-18 are the
  same guard reporting under different suppressors, not two different defects. Any future
  test-hygiene guard in this repository should be shipped together with the run that makes it fire,
  or it will silently stop guarding.

- **Insight**: The masking is asymmetric in the dangerous direction — the parallel run is green
  *because* other tests are correct, so the more of the class you fix, the more reliably the
  remainder hides.
  **Context**: That is why "the suite is green" was not evidence here, and why the detector belongs
  in CI rather than in a habit. It also explains the measurement above: 3/3 green parallel with a
  known-broken test present.

- **Insight**: `--test-threads=1` costs ~70s for the `qfs` lib half and needs no rebuild after the
  workspace suite in the same job.
  **Context**: The cheap placement is a second step inside `build-test` (shared target dir and
  `rust-cache` key), not a separate job that would recompile the workspace.
