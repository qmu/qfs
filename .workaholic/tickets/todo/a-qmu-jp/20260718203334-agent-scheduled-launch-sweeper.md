---
created_at: 2026-07-18T20:33:34+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260718203333-agent-query-functions-saved-plans.md, 20260718203332-agent-subject-policy-gate.md]
mission: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
---

# Agent cadence rides fire_due/sweep_once; the Committer seam carries the agent principal

## Overview

Let an agent with a launch cadence enter the same sweep as `/server/jobs` — no new scheduler.

Concrete work:

- An agent cadence uses the pure `qfs_watchtower::cron::fire_due` decision and the
  `LiveCronCommitter` gates (`qfs/src/sweeper.rs`): durable `last_run`, missed-fire collapse,
  skip-if-running.
- Extend the `Committer` seam to carry the firing `DecisionContext` so the policy gate evaluates
  the agent as subject (from 203332), not the operator.
- Runs append to the agent's own run-history read-back (`job_runs_schema` at
  `core/src/ddl/server.rs:800`), carrying the firing principal from 203332.
- IrreversibleGuard (RunMode::Server, Ack::Absent) refuses an irreversible plan on an agent
  cadence — fail-closed, the ruled property from the blueprint chapter.

## Policies

- Scheduling bypasses no gate: a fired agent plan is evaluated under the agent subject through the same policy + IrreversibleGuard chain.
- Hermetic-first: no wall clock in tests; use the existing injected-clock harness.
- The decision/committer split stays intact: `fire_due` stays pure.

## Quality Gate

1. Hermetic sweep tests with injected `now` fire an agent cadence and land `JobRunRecord`-shaped rows on the agent's run history.
2. A fired plan denied by the agent's policy records the denial (run history + ledger) and applies ZERO effects.
3. IrreversibleGuard refuses an irreversible plan on an agent cadence, fail-closed (RunMode::Server, Ack::Absent), asserted by test.
4. Durable `last_run` survives a simulated restart.
5. `fire_due` stays pure (decision/committer split intact).
6. Existing `/server/jobs` sweeper tests are green and unchanged.
7. Verification: `cargo test -p qfs-qfs -p qfs-watchtower` — the existing injected-clock harness, fully hermetic.

## Considerations

- Follow the blueprint chapter (20260718203330) fire-chain ruling: thread `DecisionContext` through `qfs_watchtower::Committer`; do not fork a new scheduler.
- The irreversible refusal on a timer is a RULED property, not an incidental behavior — assert it explicitly with a test, do not rely on the guard being exercised elsewhere.
- Run history reuses `job_runs_schema`; keep the agent's runs read-back beside `/server/jobs` runs.
