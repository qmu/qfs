---
created_at: 2026-08-18T13:23:05+00:00
status: done
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

## Final Report

Development completed as planned. The Overview's hypothesis was confirmed, not refuted, and the
Open Decision is resolved below.

### Step 1 — reproduction (recorded)

Probe: spawn the real `qfs serve` over `crates/server/fixtures/server_boot.qfs`, deliver the
signal after a fixed delay, record the raw wait status and which boot milestones the stderr
carried. 3–4 repetitions per cell, on this container.

| delay | SIGINT before | SIGTERM before | SIGINT after | SIGTERM after |
| --- | --- | --- | --- | --- |
| 0 ms | 130, no drain | 143, no drain | 130, no drain | 143, no drain |
| 5 ms | 130, no drain | 143, no drain | 130, no drain | 143, no drain |
| 20 ms | 130, no drain | 143, no drain | **0, drained** | **0, drained** |
| 35 ms | 130, no drain | 143 / 0 (split) | 0, drained | 0, drained |
| 50 ms | 0, drained | 0 / 143 (split) | 0, drained | 0, drained |
| 100–800 ms | 0, drained | 0, drained | 0, drained | 0, drained |

Exit 130 = `128 + SIGINT`, exit 143 = `128 + SIGTERM` — i.e. *terminated by signal*, the default
disposition, with an empty ledger. The flip sat between 35 ms and 50 ms before the change and
between 5 ms and 20 ms after it.

**A harness trap worth naming**: the first probe run reported the process *surviving* a 0 ms
SIGINT forever. That was the probe, not the product — a non-interactive shell starts every `&`
job with SIGINT/SIGQUIT set to `SIG_IGN`, and `SIG_IGN` survives `execve`, so the binary never
saw the signal. `set -m` (job control) is required to reproduce what Rust's `Command::spawn`
does, which is what the e2e suite uses.

### Step 2 — localization (recorded)

A finer probe (25–50 ms in 5 ms steps, 6 reps, logging `qfs::server` + `qfs::serve` + `qfs::http`)
correlates the outcome with the milestone reached, and the correlation is total across 72 runs:

- every death has **no** `server running` line, and reached `t36 host binding set derived` +
  the first `boot complete` (so the fixture read and the host attach are *inside* the window);
- every survival has `server running`.

`server running` is logged at `runtime.rs:378`, four lines before `shutdown_signal()` installed
the listener. So the boundary is the listener install, not the fixture read and not the listener
bind — the Overview's hypothesis, confirmed empirically rather than inferred from the source.

### Step 3 — SIGTERM behaves identically

Confirmed at every delay (the table above): raw wait status 143 in the window, exit 0 with a
drained ledger outside it. The production claim (`deploy/qfs.service`, `KillSignal=SIGTERM`) was
exposed exactly as SIGINT was.

### Step 4 — the Open Decision, resolved: **(b) arm early, defer the exit**

Rejected (a) "arm early, drain what exists": it requires inventing a meaning for `drain()` over a
half-replayed config. The audit ledger's entries *are* the boot's committed `/server` writes, so
draining mid-replay emits a partial ledger that is indistinguishable from a complete one — the
`entries=N` contract every external observer (and this suite) keys on would become
nondeterministic. `Runtime` also does not exist yet for most of the window, so there is literally
nothing to drain from.

Chose (b): install the handlers as the composition root's first act, let the kernel latch a
mid-boot signal in the already-registered handler, and have `Runtime::run` consume the latch as
its first act. `drain()` keeps its meaning, `Runtime::run` stays the single shutdown owner, and a
failed boot still exits 1 (the latch is only ever consumed by a runtime that booted).

The cost the decision names — shutdown latency becomes boot latency — is bounded and measured:
arm → `server running` is ~27 ms for the 9-statement fixture and ~110 ms for a 200-statement one,
against `TimeoutStopSec=30` in the unit file. An operator's `systemctl stop` racing a slow boot
now waits out the remaining boot instead of losing the ledger.

### Step 5–7 — what changed

- `qfs-server`: new public `ShutdownSignal` — the SIGINT/SIGTERM listeners held as a **value**, so
  installing them and awaiting them are separate moments. `ShutdownSignal::install()` registers
  both eagerly (`tokio::signal::unix`, not `ctrl_c()`, which registers only when first awaited —
  the late arming this type exists to avoid) and logs `shutdown signal armed`. A handler that
  cannot be installed degrades exactly as before (the other signal alone; neither ⇒ immediate
  shutdown), never a panic.
- `Runtime::with_shutdown(..)` adopts a pre-armed listener; `Runtime::run` uses it if present and
  otherwise arms one itself, so every non-daemon caller is untouched. `run` first does ONE
  non-blocking poll (a `biased` select against `std::future::ready`) and, if the signal already
  arrived, logs `shutdown signal arrived during boot` and drains without entering the run loop.
- `crates/qfs/src/serve.rs`: the tokio runtime is built at the TOP of `run_serve` (a signal
  handler needs a runtime context) and the listener is armed immediately, before engine
  construction, driver registration, the System-DB session store, the daemon host, the OAuth AS
  and the config replay. The armed listener rides into `Runtime::with_shared(..)`.
- `crates/http/src/serve.rs`: the single-shutdown-owner comment now says that arming early adds no
  second owner — only `run` ever consumes the latch.
- `deploy/qfs.service`: the graceful-shutdown comment states the contract now holds during boot,
  and that `TimeoutStopSec` is the bound on a stop that races one.
- `run()`'s doc comment gained a `…and it holds while the daemon is still BOOTING` section stating
  the contract the code now actually holds.
- New e2e scenario 10 (`crates/cmd/tests/e2e_binding_ddl.rs`) — SIGINT and SIGTERM twins that wait
  for `shutdown signal armed` on the child's stderr, signal there, and assert exit 0, the
  `shutdown signal arrived during boot` marker, `boot complete`, and `entries=200`. The boot they
  signal into is a generated 200-statement fixture, so ~110 ms of replay stands against the ~5 ms
  a `kill` spawn costs — a margin that is deterministic by construction rather than hopeful.
- New shared test support `crates/cmd/tests/serve_e2e/mod.rs` — `ServeLog`, a bounded readiness
  wait over a pumped stderr. The sibling ticket's sweep of the four `sleep`-then-signal sites
  reuses it.

### Where the contract still stops, stated plainly

The Quality Gate asks for "any point after process start", and the Gate asks the probe to pass at
every delay it previously failed at. **0 ms and 5 ms still fail** and no design closes them: a
handler cannot be installed before the process's first instruction. The measured residual is
~12 ms from `execve` to the arm, of which ~4.5 ms is loader + Rust init (irreducible — `qfs
--version`, which returns before any other work, costs that much) and ~7 ms is `main`'s
`store::open_system_db()` plus the clap parse.

Closing that ~7 ms was considered and **not** done: it needs either an argv sniff in `main` plus a
process-global stash for the pre-built runtime and the armed listener, or a changed
`qfs_cmd::ServeLauncher` signature. Both trade this codebase's composition-root discipline for a
window that stays ≥4.5 ms regardless. What the defect actually was — the *config-dependent* part
of boot, which grows with the deployment's config size, its driver registrations and its disk — is
fully closed. The residual is a constant, and it is the developer's call whether it is worth the
wart; the PR's Concerns carries it.

### Discovered Insights

- **Insight**: `tokio::signal::ctrl_c()` registers its handler lazily, at the first `.await` —
  so "the process listens for SIGINT" is not a property of linking the listener but of *reaching*
  the await. Any daemon that awaits it after boot is uncovered for the whole of boot.
  **Context**: this is the entire mechanism of the flake, and it is invisible in the source: the
  call site reads like a wait, not like an installation. `tokio::signal::unix::signal(..)` is the
  eager form, which is why `ShutdownSignal` uses it even for SIGINT, where `ctrl_c()` would
  otherwise be the idiomatic choice.
- **Insight**: `Signal::recv` is cancel-safe and the registration lives in the `Signal` value, so
  a *non-blocking poll* of a pre-armed listener is free — `tokio::select!` with `biased` against
  `std::future::ready(())` gives "has it already fired?" with no extra dependency and no cost to
  the wait that follows.
  **Context**: that is what lets `Runtime::run` stay the single drain owner while still reacting
  to something that happened before it started running. The alternative — a separate `AtomicBool`
  latch written by a second task — would have been a second shutdown owner.
- **Insight**: a non-interactive shell sets `SIGINT`/`SIGQUIT` to `SIG_IGN` for every `&` job, and
  `SIG_IGN` survives `execve`. A bash reproduction of a signal-handling defect silently tests
  nothing unless it enables job control (`set -m`).
  **Context**: cost ~10 minutes of a hung probe and a wrong first reading (the process appeared to
  *ignore* SIGINT). Any future signal probe in this repository should start with `set -m`.
- **Insight**: `qfs serve` logs `boot complete` **twice** — once from `attach_daemon_host`'s
  in-memory re-boot that projects the binding set, and once from the runtime's own replay.
  **Context**: any readiness wait or assertion keyed on `boot complete` is ambiguous about which
  boot it saw; `server running` (now genuinely armed-and-waiting) is the unambiguous line.
