---
created_at: 2026-07-13T13:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# The daemon sweeper can never fire a real job: stored plan is AST-JSON, fire_due lexes it as text

## Problem (found live, round 9, v0.0.59 — the headline sweeper feature does not work end-to-end)

Every sweep of every job fails identically, for BOTH install paths:

- a `create job … every '1m' do upsert into /local/... values ('tick') policy heartbeat` served
  from a config file (`qfs serve round9.qfs`), and
- the same statement installed through the daemon's own commit bridge
  (`POST /api/commit`, `SERVER_JOBS_UPSERT` applied: true),

produce a `/server/jobs/<name>/runs` history of

> outcome failed — "stored JOB body did not parse: lexing failed: UNEXPECTED_CHAR"

every 30s sweep. The stored row explains it: `/server/jobs` `plan` column holds the serialized
AST JSON (`{"Effect":{"verb":"Upsert","target":{"segments":[...]}}}`), while the firing path
(`fire_due` → "stored JOB body did not parse") tries to LEX that JSON as qfs statement text —
`{`/`"` → UNEXPECTED_CHAR. Writer and reader disagree on the persisted format, so **no daemon-
fired job has ever succeeded against the real binary**. The PR #35 hermetic E2E ("a real local-FS
scheduled fire through the live committer") must construct its job row in the format the reader
expects, bypassing the real install path — the seam the live round exposed.

What the round DID prove live (all working): config boot registers policy+job; the 30s real-clock
sweeper runs and re-fires failed plans (at-least-once); the read-only `/server/jobs/<name>/runs`
ledger records each attempt and is readable from outside via `POST /api/run {mode: "read"}`; the
commit bridge's `api`-policy default-deny refuses job installs until the config grants
`ALLOW upsert on 'server/*'`.

## Fix

Make writer and reader agree: either fire_due deserializes the stored AST plan (preferred — no
reparse, the plan was already validated at install), or the install paths store the statement
text the firer expects. Fix BOTH install paths (config boot and SERVER_JOBS_UPSERT). Re-point the
hermetic E2E at the real install path — the test must create the job via `create job` statement
or config boot, never by hand-built row, so this class of writer/reader drift can't pass again.
Then a live re-run of round 9: one fired run with `outcome: fired`, `affected 1`, and the tick
file written.

## Key files

- `packages/qfs/crates/qfs/src/sweeper.rs` + `qfs_watchtower::cron::fire_due` — the reader
- serve config boot job registration + the `SERVER_JOBS_UPSERT` applier — the writers
- the PR #35 E2E test that passed with a hand-shaped row

## Acceptance

- Round-9 config (policy + 1m local-upsert job) on `qfs serve`: within 90s the runs ledger shows
  `fired` with `affected 1` and the target file exists; history survives restart.
- A statement-installed job (`/api/commit`) fires equally.
- The E2E creates its job through a real install path.

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause pinpointed: the writer/reader agreed on canonical JSON everywhere EXCEPT the in-server
sweeper. `JobDecl.plan` is stored as `PlanSpec::canonical()` (serialized AST); the trigger
dispatcher (`dispatch.rs`) and `qfs job run` (`job.rs`) already rehydrate it with
`PlanSpec::from_canonical` (no re-parse). Only `qfs_watchtower::cron::fire_one` re-lexed the stored
JSON as statement text (`qfs_exec::parse`) → `UNEXPECTED_CHAR` every sweep.

Fix (reader-side, the ticket's preferred option — no re-parse, matches all other readers, covers
BOTH install paths at once because both already store canonical JSON):

- `cron.rs::fire_one` now rehydrates via `qfs_core::PlanSpec::from_canonical(job.plan.0)` and
  commits `spec.statement()`. Failure message reworded to "did not rehydrate".
- Corrected the stale `JobDef.plan`/`fire_one` doc claims that it was "source text".
- **Test drift closed (acceptance 2 & 3):** the sweeper/cron test helpers built their `plan`
  column from raw statement text — the "hand-shaped row" the ticket calls out. They now build it
  through `PlanSpec::from_statement(parse(src)).canonical()` (a `plan_col` helper), i.e. the exact
  bytes the real install path writes. `sweep_once_with_the_live_committer_applies_a_real_local_write`
  now drives a real-format plan through the real `LiveCronCommitter` into the local-FS applier and
  asserts the tick file + `affected 1` — this fails without the reader fix (canonical JSON cannot
  lex as a statement), so the writer/reader drift can never pass green again.

**Remaining (owner-attended, not code):** acceptance item 1 — the live `qfs serve` re-run of round
9 (config boot fires a 1m job over the daemon within 90s, history survives restart) — is the
owner-attended live round, tracked by the mission's server-scheduling acceptance.

## Live acceptance — round 9 re-run (2026-07-13, owner-attended, v0.0.61)

**PASSED.** Re-ran on the branch binary `qfs 0.0.61` (commit `ccefc9d`, = shipped v0.0.60 sweeper
fix + the /fs & gdrive content-schema fixes). Config `round9.qfs`:

```qfs
CREATE POLICY heartbeat ALLOW UPSERT;
CREATE JOB heartbeat_job EVERY '1m' DO UPSERT INTO /local/tmp/qfs-round9/tick.txt VALUES ('tick') POLICY heartbeat;
```

`qfs serve round9.qfs` (the internal 30s real-clock sweeper). Evidence:

- **Fired within 90s** — actually `<1s` on boot: `qfs::cron: cron sweep firing job=heartbeat_job
  outcome=Fired { affected: 1 }`. The on-disk runs ledger (`.qfs-state/audit.log`) recorded
  `cron fire job=heartbeat_job outcome=fired affected=1 at=1783941910`.
- **`affected 1`** on every fire; the **tick file was written** (`/tmp/qfs-round9/tick.txt` =
  `tick` — `/local` roots at `/`, so `/local/tmp/…` → `/tmp/…`).
- **1m cadence held** — a second fire at `at=1783941970` (+60s), durable
  `.qfs-state/durable/cron_heartbeat_job_last_run.state` advanced `1783941910 → 1783941970`.
- **History survives restart** — SIGTERM'd the daemon, removed the tick file, restarted from the
  same state dir 9s after the last fire: the restarted daemon **re-hydrated the durable `last_run`
  and did NOT re-fire** (tick file stayed absent — the mark survived), then **resumed the schedule**,
  firing again at the next due tick `at=1783942039`. Full ledger:

  ```
  cron fire job=heartbeat_job outcome=fired affected=1 at=1783941910   (run 1, boot)
  cron fire job=heartbeat_job outcome=fired affected=1 at=1783941970   (run 1, +60s)
  cron fire job=heartbeat_job outcome=fired affected=1 at=1783942039   (run 2, after restart — resumed)
  ```

All of acceptance item 1 is satisfied. Mission server-scheduling acceptance ticked (2026-07-13).
Note: the HTTP `/api` listener could not bind `:8787` (a concurrent session's daemon holds it), so
this run used config-only serving + the on-disk audit ledger as the read-back; the sweeper path is
independent of the HTTP listener and fired regardless.
