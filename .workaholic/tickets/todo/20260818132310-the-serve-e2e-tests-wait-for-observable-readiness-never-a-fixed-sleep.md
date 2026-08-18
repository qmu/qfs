---
created_at: 2026-08-18T13:23:10+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: a-shutdown-signal-is-graceful-from-the-first-instant-of-boot
merge_policy:
verification_handoff: 
---

# The serve e2e tests wait for observable readiness, never a fixed sleep

## Overview

The flake reported on PR #74's merge commit is one instance of a pattern the serve e2e suite
uses in five places: spawn `qfs serve`, `sleep` a guessed interval, assert the child has not
exited, then signal it. The sleep is a guess about how long boot takes, and the liveness
assertion does not distinguish "booted and waiting in the run loop" from "still booting" — a
process that has not exited satisfies it either way. On a loaded CI runner the guess is wrong
and the signal lands too early.

The sites:

| File | Line | Guess |
| --- | --- | --- |
| `crates/cmd/tests/e2e_binding_ddl.rs` | 677 | `sleep(800ms)` then SIGINT — the one that failed |
| `crates/cmd/tests/e2e_serve.rs` | 122 | `sleep(800ms)` then SIGINT |
| `crates/cmd/tests/e2e_serve.rs` | 189 | `sleep(800ms)` then SIGTERM |
| `crates/cmd/tests/e2e_serve.rs` | 240 | `sleep(700ms)` then SIGINT (clean-env boot) |
| `crates/cmd/tests/e2e_serve.rs` | 616 | `sleep(100ms)` retry loop — already a real wait (binds), listed for contrast |

Fixing only the reported test leaves the other three able to redden a merge the same way, so
this ticket sweeps the family rather than the instance.

The server's stderr already carries the observable signals — `boot complete` and
`server running` (`crates/server/src/runtime.rs:305`, `:378`) — and each of these tests already
pipes stderr and asserts on those strings *after* the process exits. Reading stderr
incrementally until the readiness line appears, with a bounded timeout, replaces the guess with
an observation.

**Depends on the sibling ticket**
(`a-shutdown-signal-arriving-during-boot-takes-the-graceful-path`): which line means "ready to
handle a signal" is settled there, because today `server running` is logged *before* the handler
is installed — so waiting on it narrows the window without closing it. This ticket makes the
harness deterministic given a readiness signal that is true; it does not make the signal true.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout

## Key Files

- `packages/qfs/crates/cmd/tests/e2e_serve.rs` — three sleep-then-signal sites (`:122`, `:189`,
  `:240`) plus `send_sigint`/`send_signal` (`:88`) and the bind-retry loop at `:616` that shows
  the shape a real wait already takes in this suite.
- `packages/qfs/crates/cmd/tests/e2e_binding_ddl.rs` — the reported site at `:660`–`:711`.
- `packages/qfs/crates/server/src/runtime.rs` — `:305` `boot complete`, `:378` `server running`:
  the lines a readiness wait keys on, and the reason the sibling ticket decides which.

## Implementation Steps

1. **Reproduce the class, not just the instance.** Run the four sleep-then-signal tests under
   artificial load (or with the sleep cut to a few ms) and confirm each fails the same way —
   evidence that the pattern, not one test, is the defect.
2. Add one shared helper to the e2e test support — read the child's stderr line by line until
   the readiness line appears, returning the lines consumed, with a **bounded timeout that
   fails loudly** naming what it waited for and what it saw. Keep the consumed lines available,
   since every one of these tests asserts on `boot complete` / `entries=N` afterwards.
3. Replace each `sleep`-then-`try_wait` guess with that helper, keeping the existing
   "must not have self-exited" assertion where it still says something.
4. Key the helper on the readiness line the sibling ticket settles on; if that ticket lands
   first, use its line — otherwise gate this one behind it rather than shipping a wait on a
   line that does not yet mean what it says.
5. Re-run the suite repeatedly (a loop of N runs, and under `--test-threads` pressure) to show
   the flake does not reappear.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- No serve e2e test decides readiness with a fixed `sleep`; each waits on an observable
  readiness line.
- Every readiness wait has a bounded timeout whose failure message names the line awaited and
  the stderr actually seen.
- The stderr consumed while waiting is still available to the assertions that follow, so no
  existing assertion is weakened to accommodate the wait.

**Verification method** — the commands/tests/probes that prove them:

- `cd packages/qfs && cargo test --workspace` green.
- A repeat loop over the four affected tests (e.g. 20 consecutive runs, and once under CPU
  load) with no failure.
- `rg 'sleep\(Duration::from_millis' crates/cmd/tests/` shows no remaining readiness guess
  (poll intervals inside a bounded wait loop are fine and are the intended shape).

**Gate** — what must pass before approval:

- Step 1's reproduction is recorded for each of the four sites, and each passes the repeat loop
  after the change.

## Considerations

- **Ordering.** Landing this before the sibling ticket buys a narrower window, not a closed one:
  `server running` is logged before the handler is armed, so a wait on it can still be beaten.
  Say so in the Final Report if it ships first, rather than reporting the flake as fixed.
- A stderr-blocking read must not deadlock if the child never reaches the line — the timeout is
  the whole safety of the change, and a helper that hangs replaces a flaky red with a stuck job.
- `e2e_serve.rs:616` already polls a real condition (the port binding); leave it, and reuse its
  shape rather than inventing a second one.
