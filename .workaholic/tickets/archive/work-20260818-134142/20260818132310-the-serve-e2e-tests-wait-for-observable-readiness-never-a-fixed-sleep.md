---
created_at: 2026-08-18T13:23:10+00:00
status: done
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

## Final Report

Development completed as planned. The sibling ticket
(`20260818132305-a-shutdown-signal-arriving-during-boot-takes-the-graceful-path`) landed first in
this same unit, so step 4's "otherwise gate this one behind it" branch did not apply: the wait is
keyed on a readiness line that now means what it says.

### Step 1 — the class reproduced, per site

Every `sleep`-then-signal site was cut to `sleep(1ms)` and the suite run **against the already
fixed binary**. All four fail, each in its own way, which is the evidence that the pattern rather
than one test is the defect:

| Site | Test | Failure with a 1 ms guess |
| --- | --- | --- |
| `e2e_binding_ddl.rs:677` | `serve_boots_mixed_fixture_and_drains_audit_on_sigint` | `clean shutdown on SIGINT must exit 0, got ExitStatus(unix_wait_status(2))` |
| `e2e_serve.rs:122` | `serve_boots_blocks_then_sigint_drains_audit_cleanly` | `clean shutdown on SIGINT must exit 0, got ExitStatus(unix_wait_status(2))` |
| `e2e_serve.rs:189` | `serve_shuts_down_cleanly_on_sigterm_and_drains_audit` | `clean shutdown on SIGTERM must exit 0 (not 143/uncaught), got ExitStatus(unix_wait_status(15))` |
| `e2e_serve.rs:240` | `serve_boots_without_network_or_credentials` | `boot succeeds with a cleared environment (no network/creds)` — no `boot complete` in the log |

The first line is verbatim the failure PR #74's merge commit produced. Note what the liveness
assertion did **not** catch in any of the four: the child had not exited at 1 ms, so
`try_wait().is_none()` passed everywhere — "has not exited" never distinguished "in the run loop"
from "still booting", exactly as the Overview says.

### Step 2 — the shared helper

`crates/cmd/tests/serve_e2e/mod.rs` (created in this unit's first commit for scenario 10, extended
here) carries:

- `ServeLog::pump(stderr)` — a background thread draining the child's stderr into a buffer, so a
  child that never prints the awaited line cannot block the test; the deadline belongs to the
  waiter, not to the read (the Considerations' "must not deadlock" requirement).
- `ServeLog::wait_for(needle, timeout)` — polls the buffer every 5 ms, returns the log as of the
  match, and on timeout panics naming **the line awaited**, whether the child's stderr is still
  open, and the full text it did see.
- `ServeLog::finish()` — joins the pump after the child exits and returns the complete log, so
  every assertion downstream (`boot complete`, `entries=9`, the per-entry audit-line count) reads
  the same evidence it always did. Waiting costs no assertion.
- `SERVE_READY` / `SHUTDOWN_ARMED` — the two readiness lines named once, with the reason each
  means what it means; `READINESS_TIMEOUT` (30 s) and `SHUTDOWN_TIMEOUT` (10 s) as the two budgets.
- `wait_for_exit(child, after)` — the bounded post-signal wait, previously copy-pasted three times.

### Step 3–4 — the sweep

All four guesses are gone; each site now waits on `SERVE_READY` (`server running`) — the line the
sibling ticket made true by arming the handlers before boot. The "must not have self-exited"
assertion is kept at the three sites where it still says something (a crash between the readiness
line and the signal). The clean-env test additionally asserts its exit status now, which it
silently discarded before.

`e2e_serve.rs:604`'s bind-retry loop is untouched: it already polls a real condition, and its
100 ms interval is a poll inside a bounded wait, which is the intended shape. It is the only
`sleep(Duration::from_millis` left under `crates/cmd/tests/`.

### Step 5 — the repeat loop

- 20 consecutive runs of the four affected tests plus scenario 10's two: all green.
- 8 consecutive runs under 3× oversubscription (12 spinners on 4 cores): all green, with wall
  time rising from ~0.2 s to ~0.5 s per run — i.e. the tests genuinely slowed down and still
  passed, which is the load condition the fixed sleeps failed under.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`: clean.

The suite also got faster: the two files' serve scenarios drop from ~0.86 s each to ~0.16–0.22 s,
because a real wait finishes when boot finishes instead of always paying 800 ms.

### Discovered Insights

- **Insight**: the four sites' liveness assertion (`try_wait().is_none()`) passed under every
  reproduction, including the ones where the signal then killed the process. A "has not exited"
  check can never be a readiness check — it is satisfied identically by a booted process and by a
  process that is 1 ms old.
  **Context**: this is why cutting the sleep produced *shutdown* failures rather than *liveness*
  failures, and why raising the sleep would only have moved the flake rather than removing it.
- **Insight**: `cargo test` runs a file's tests concurrently by default, so several `qfs serve`
  children coexist; they all try to bind `127.0.0.1:8787` and all but one log
  `http listener could not bind … Address already in use`. That is non-fatal by design (boot
  continues without the listener), which is why these tests never noticed.
  **Context**: any future serve e2e that asserts on the HTTP listener must pass its own
  `QFS_HTTP_ADDR`, as the `/claude` sessions scenario already does — an assertion on
  `http listener bound` would be flaky for a reason unrelated to timing.
