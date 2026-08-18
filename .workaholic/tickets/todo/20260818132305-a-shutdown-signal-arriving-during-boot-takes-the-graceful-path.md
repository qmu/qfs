---
created_at: 2026-08-18T13:23:05+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: a-shutdown-signal-is-graceful-from-the-first-instant-of-boot
merge_policy:
verification_handoff: 
---

# A shutdown signal arriving during boot takes the graceful path

## Overview

PR #74's merge commit reddened `main`: `serve_boots_mixed_fixture_and_drains_audit_on_sigint`
(`packages/qfs/crates/cmd/tests/e2e_binding_ddl.rs:696`) failed with

```
clean shutdown on SIGINT must exit 0, got ExitStatus(unix_wait_status(2))
```

while every other job in that run was green, and the next push over the same tree passed
(runs `32139568841` → `32139615576`). Raw wait status `2` is *terminated by signal 2*, not
*exit code 2* — the process met SIGINT at its default disposition rather than in a handler.

The reporter offered a fork: make the test wait for demonstrable readiness, or — if that
ordering cannot be made deterministic from outside the process — decide what the test should
assert instead. A first reading of the source says the second branch applies, and the reason
is a product property rather than a test one: the listener is installed by
`shutdown_signal()` *inside* `Runtime::run()` (`crates/server/src/runtime.rs:382`), after
boot and after the `server running` line at `:378` that any external readiness wait would key
on. Between process start and that install, SIGINT and SIGTERM both kill the process
un-drained — so the `t36` contract stated in `run()`'s own doc comment ("a `systemctl stop`
is a clean drain, not an uncaught SIGTERM (exit 143 with no drain)") does not hold while the
daemon is still booting. The e2e flake is that gap seen from outside, and a `systemctl stop`
racing a slow boot is the same gap in production.

This ticket owns the process side. The sibling ticket
(`the-serve-e2e-tests-wait-for-observable-readiness-never-a-fixed-sleep`) owns the harness
side, and depends on whatever readiness signal this one settles on.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:operation` — runtime behavior and recovery: a signal is a delivery-path contract,
  and `deploy/qfs.service` (`KillSignal=SIGTERM`) is the operator surface this defends

## Key Files

- `packages/qfs/crates/server/src/runtime.rs` — `Runtime::run()` (`:377`) logs `server running`
  at `:378` and only then calls `shutdown_signal()` at `:382`; the window is between process
  start and that call. Its doc comment carries the `t36` SIGTERM contract to be corrected or
  made true.
- `packages/qfs/crates/qfs/src/serve.rs` — the `qfs serve` entrypoint (`0` clean shutdown, `1`
  on boot/bind/runtime error); wherever the signal is armed earlier, it is armed here or above
  `Runtime` boot.
- `packages/qfs/crates/server/src/audit.rs` — `drain()` is what a graceful path must still run;
  what it means to drain a ledger from a partially booted runtime is the open decision below.
- `packages/qfs/crates/http/src/serve.rs:161` — the comment recording that `Runtime::run` OWNS
  the single `ctrl_c` shutdown and must run to completion; any earlier arming must not create a
  second owner racing this one.
- `packages/qfs/crates/cmd/tests/e2e_binding_ddl.rs:660` — the failing test, and the first place
  a mid-boot signal can be exercised.
- `deploy/qfs.service` — the `KillSignal=SIGTERM` this makes true during boot.

## Implementation Steps

**Diagnosis first — the reading above is a hypothesis, not the design.** Do not start from the
mechanism; start from the failure.

1. **Reproduce.** Drive `qfs serve` over the fixture and send SIGINT at decreasing delays
   (e.g. 0/50/200/800 ms after spawn), recording the raw wait status at each. Confirm that an
   early signal yields `unix_wait_status(2)` and a late one exit 0, and record the delay at
   which behavior flips on this machine.
2. **Localize.** Confirm the window empirically — that the flip coincides with the
   `shutdown_signal()` install rather than with, say, listener bind or a fixture read. Instrument
   or bisect; do not infer it from the source alone.
3. **Confirm SIGTERM behaves identically**, since the production claim (`systemctl stop`) rides
   on SIGTERM and only SIGINT was observed in CI.
4. **Resolve the open decision below** and record the resolution in the Final Report.
5. **Implement** the chosen arming, keeping `Runtime::run` the single owner of the drain
   (`crates/http/src/serve.rs:161`) — one shutdown owner, not two racing.
6. **Cover it with a test that signals mid-boot**, at a delay proven in step 1 to land inside
   the window, asserting exit 0 and a drained ledger.
7. **Correct or fulfil the `t36` doc comment** so its stated contract and the code agree for the
   whole process lifetime, boot included.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A SIGINT or SIGTERM delivered at any point after process start — including inside the window
  measured in step 1 — exits 0 and drains the audit ledger.
- `Runtime::run` remains the single owner of the drain; no second shutdown path can run it.
- `run()`'s doc comment states a contract the code actually holds during boot.

**Verification method** — the commands/tests/probes that prove them:

- A new e2e test signalling mid-boot at the measured delay, asserting exit 0 and
  `audit ledger drained`.
- `cd packages/qfs && cargo test --workspace` — including
  `serve_boots_mixed_fixture_and_drains_audit_on_sigint` and the `e2e_serve.rs` SIGTERM test.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`.

**Gate** — what must pass before approval:

- The reproduction from step 1 is recorded (delays and raw statuses), and the same probe passes
  after the change at every delay it previously failed at.

## Open Decisions

<!-- A fork this proposing session cannot recommend one side of; the driving session
     resolves it explicitly and records the resolution in its Final Report. -->

- **What a mid-boot shutdown should do with a half-built runtime.** Two answers, and they are
  not equivalent for a production `systemctl stop` racing a slow boot:
  (a) **arm early, drain what exists** — install the listener before boot so a mid-boot signal
  runs the graceful path over a partially constructed `Runtime`, which requires defining what
  "drain" means when the ledger and bindings are incomplete; or
  (b) **arm early, defer the exit** — block/latch the signal during boot and act on it at the
  first point the runtime is whole, which keeps `drain()`'s meaning intact but makes shutdown
  latency depend on boot time (an operator's `systemctl stop` waits out a slow boot, and
  systemd's `TimeoutStopSec` then decides).
  The reporter delegated this explicitly ("decide explicitly what the test should assert
  instead"), so it is recorded rather than resolved here.

## Considerations

- The mechanism above is the **reporter's and this proposal's hypothesis**. Step 1 exists to
  confirm or refute it; if the flip does not coincide with the `shutdown_signal()` install, the
  design that follows changes and this ticket's Overview is the thing that was wrong.
- A signal handler armed before boot must not swallow the `1` exit path for a boot/bind error
  (`crates/qfs/src/serve.rs:20`) — a failed boot must still fail, not report a clean shutdown.
- Whether the readiness signal an external harness can key on should also move (e.g. the
  `server running` line emitted only once the handler is armed) is decided here and consumed by
  the sibling ticket; that ordering is what makes the harness fix deterministic rather than
  merely wider.
