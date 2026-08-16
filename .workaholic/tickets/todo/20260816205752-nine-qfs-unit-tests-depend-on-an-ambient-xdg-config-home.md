---
created_at: 2026-08-16T20:57:52+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260816-210358
---

# Nine `qfs` unit tests fail on a checkout where `XDG_CONFIG_HOME` is unset

## Overview

`packages/qfs/crates/qfs/src/store.rs` carries a guard, `forbid_shared_home_fallback_in_tests`,
whose job is to stop a unit test from resolving the shared `$HOME/.config/qfs` store. It fires by
panicking, and its message names the fix: *"wrap the test in `testenv::HomeGuard` so it uses an
isolated config home"*. Nine tests in the `qfs` crate never took that wrap, so they pass only where
the **ambient environment** happens to set `XDG_CONFIG_HOME` and fail everywhere else.

Measured 2026-08-16 in a fresh cloud container while driving `20260816183441`:

```
$ cd packages/qfs && cargo test --workspace
test result: FAILED. 474 passed; 9 failed; 1 ignored
thread '…' panicked at crates/qfs/src/store.rs:54:5:
the store resolved to the shared $HOME/.config/qfs (XDG_CONFIG_HOME unset) inside a qfs unit test
— wrap the test in `testenv::HomeGuard` so it uses an isolated config home

$ XDG_CONFIG_HOME=/tmp/xdg cargo test --workspace
test result: ok. 2723 passed; 0 failed
```

The nine:

- `declared_driver::tests::declared_secret_ref_store_rejects_a_different_auth_key`
- `declared_driver::tests::declared_secret_ref_store_resolves_env_secret_for_default_auth`
- `declared_driver::tests::declared_secrets_builds_the_account_adapter_for_account_auth`
- `sweeper::tests::sweep_once_agent_fire_denied_by_agent_subject_records_denial`
- `sweeper::tests::sweep_once_agent_irreversible_is_blocked_fail_closed`
- `sweeper::tests::live_round_rehearsal_narrow_grant_fires_and_overreach_is_denied`
- `sweeper::tests::sweep_once_with_the_live_committer_applies_a_real_local_write`
- `sweeper::tests::live_committer_gates_deny_and_block_before_any_apply`
- `telemetry::tests::sink_selection_builds_the_configured_sink`

`CLAUDE.md` advertises the suite as "all hermetic (no network/credentials)". These nine are hermetic
about the network and not about the environment, and the difference is invisible until a machine
without the variable runs them — where the red gate reads as a defect in whatever change is being
driven, which is exactly how it was found.

## Scope

**In scope:** make the nine tests hermetic — wrap each in `testenv::HomeGuard` (or the equivalent
isolation the guard's message names), so `cargo test --workspace` is green with `XDG_CONFIG_HOME`
unset.

**Out of scope:**

- The guard itself. It did its job: it refused rather than silently writing into a developer's real
  config store, which is the behaviour to keep.
- Setting `XDG_CONFIG_HOME` in CI or a runner image. That hides the defect at the one place it is
  currently visible instead of fixing it, and leaves a local `cargo test` red.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — all code work.
- `workaholic:implementation` / `policies/directory-structure.md` — all code work.
- `workaholic:operation` / `observability` — a test suite whose result depends on an unstated
  ambient variable reports something other than the state of the code.

## Key Files

- `packages/qfs/crates/qfs/src/store.rs` (~54) — `forbid_shared_home_fallback_in_tests`, the guard
  and its message.
- `packages/qfs/crates/qfs/src/declared_driver.rs`, `src/sweeper.rs`, `src/telemetry.rs` — the three
  test modules holding the nine unguarded tests.
- `packages/qfs/crates/qfs/src/testenv.rs` — `HomeGuard`, the isolation the guard's message names.
- `CLAUDE.md` — the "all hermetic" claim the fix restores.

## Implementation Steps

1. Read `testenv::HomeGuard` and how the already-guarded tests in the same modules use it.
2. Wrap each of the nine; prefer the existing local idiom over a new helper.
3. Run `cargo test --workspace` with `XDG_CONFIG_HOME` **unset** and confirm green.
4. Re-run with it set, so the fix is not merely swapping which environment passes.

## Quality Gate

**Acceptance criteria**

- `env -u XDG_CONFIG_HOME cargo test --workspace` exits 0 from `packages/qfs`.
- `XDG_CONFIG_HOME=<tmp> cargo test --workspace` still exits 0.
- The `store.rs` guard is unchanged — the tests move to isolation, the guard does not relax.

**Verification method**

- Both commands above, run in the worktree, with the `test result:` lines pasted into the ticket
  outcome rather than paraphrased.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` all exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-16 while driving `20260816183441`
  (`EvalError` `Display`). The nine failures were unrelated to that change — confirmed by re-running
  the same suite with `XDG_CONFIG_HOME` set, which turned all 2723 green — so they were reported
  rather than fixed opportunistically.
- Worth checking whether the guard can name the *test* it fired in, not only the fix: nine
  identical panics in one run cost a bisect to attribute.
