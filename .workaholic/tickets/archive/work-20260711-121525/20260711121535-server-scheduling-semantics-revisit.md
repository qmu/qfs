---
created_at: 2026-07-11T12:15:35+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Server scheduling semantics — deliberately reverse t65 and ship in-server CREATE JOB firing

## Overview

The owner has changed their mind: **qfs needs scheduling semantics** ("Changed mind, we need
this" — mission directive, 2026-07-11). This deliberately reverses the settled t65 decision
("qfs is not a scheduler": scheduling externalized to OS cron / CF Cron Triggers, e188846), so
the ticket starts by recording the reversal in the blueprint with the reasoning, then ships the
semantics. The substrate half-exists: `CREATE JOB` DDL desugars to `/server/jobs` rows,
`BindingKind::Cron` participates in the declarative reconcile seam, and the watchtower dispatch
loop (EventBus → gate → commit) is live — the missing piece is the **cron fire-path leaf**
(t32-class work): a daemon-side timer that evaluates each job's schedule, emits the event,
executes the bound statement under policy, with at-least-once + idempotency and observable
run history.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/observability.md` — the core lens: timeouts, bounded retries, run-history rows, structured spans per firing; a wedged job must be visible and recoverable
- `workaholic:design` / `policies/access-control.md` — a job fires under its declared policy grant only; no ambient authority
- `workaholic:design` / `policies/defense-in-depth.md` — the firing path re-checks the policy gate and the irreversible gate independently; scheduling never bypasses either

## Key Files

- `packages/qfs/crates/server/src/binding.rs` - BindingKind::Cron + the reconcile seam the timer converges to
- `packages/qfs/crates/core/src/ddl/server.rs` - CREATE JOB desugaring to /server/jobs rows
- `packages/qfs/crates/watchtower/src/lib.rs` - EventBus → trigger → gated commit dispatch, at-least-once + idempotency ledger
- `packages/qfs/crates/server/src/lib.rs` - ServerState registries (jobs) and reconcile driver
- `docs/blueprint.md` - the t65 externalization decision to reverse, §10 automation chapter

## Related History

The reversal target and the substrate both have recorded history; provisioning-reconcile is the config model jobs live under.

- [20260626102300-t65-externalize-scheduling.md](.workaholic/tickets/archive/work-20260628-000332/20260626102300-t65-externalize-scheduling.md) - the settled "qfs is not a scheduler" decision this ticket explicitly reverses
- [20260708004800-provisioning-reconcile-implementation.md](.workaholic/tickets/archive/work-20260707-180554/20260708004800-provisioning-reconcile-implementation.md) - qfs plan/apply reconcile; jobs are rows in this universe

## Implementation Steps

1. **Record the reversal**: blueprint decision entry — why external cron proved insufficient (the owner's grounds: qfs server owns triggers/endpoints already; scheduling is the missing "when" beside the shipped "what"), what t65 got right (no scheduler *library*, no wall-clock in the pure core), and the new ruling's scope (daemon-only; wasm/workers hosts keep delegating to platform cron).
2. Implement the cron fire-path leaf behind `host-daemon`: a tokio timer task per reconciled Cron binding (or one sweeper), schedule parsing (reuse a minimal cron-expression parse — implement in-house per vendor-neutrality unless a criterion is clearly met and logged in §11), firing = emit the job event through the existing watchtower dispatch (gate → policy → commit).
3. Run history: append per-firing rows (job, scheduled_at, started/finished, outcome, error) to a `/server` collection so `SELECT FROM /server/jobs/<name>/runs`-class reads answer "did it run".
4. Semantics to rule and test: missed-fire policy on daemon restart (skip vs catch-up — rule it), overlap policy (skip-if-running default), timezone (UTC only, stated), at-least-once + idempotency via the existing ledger.
5. Hermetic tests with an injected clock: firing at schedule, no-fire outside, restart/missed-fire behavior, overlap skip, policy-denied job records a denied run. Docs: server reference (gen-docs), automation cookbook, gen-skills; plugin version bump (taught surface).

## Quality Gate

**Acceptance criteria**

- CREATE JOB with a schedule reconciles into a live Cron binding that fires under an injected clock in tests.
- Every firing is policy-gated and irreversible-gated independently; a denied firing leaves a visible denied run row.
- Missed-fire, overlap, and timezone semantics are ruled, documented, and each covered by a test.
- The blueprint carries the explicit t65-reversal decision entry.

**Verification method**

- `cargo test --workspace` green (injected-clock scheduler tests, watchtower dispatch, reconcile); `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; the live round (a real daemon firing a harmless job on a 1-minute schedule, run history read back) runs owner-attended and is recorded on this ticket.

## Considerations

- Wall-clock stays out of the pure core: the timer and `now()` live in the daemon leaf; plan/eval remain clock-free (`packages/qfs/crates/watchtower/src/lib.rs` tokio feature gating is the pattern)
- Cron-expression parsing in-house vs crate is a §11 decision-log entry either way (`docs/blueprint.md`)
- CF Workers host keeps platform Cron Triggers as its "when" — the reversal is daemon-scoped, not a new portability burden (`packages/qfs/crates/host/Cargo.toml`)

## Live Round Evidence

### Round 9 — qfs serve firing a 1-minute JOB (2026-07-13, owner-attended, FAILED — blocking defect found)

- **Binary:** qfs 0.0.59 (c30fa0a). Config: `create policy heartbeat ALLOW upsert on 'local/*'` +
  `create job heartbeat every '1m' do upsert into /local/home/ec2-user/qfs-round9-tick.txt values
  ('tick') policy heartbeat`, served with `QFS_HTTP_ADDR=127.0.0.1:8797` (8787 was taken by an
  unrelated workerd).
- **The blocking defect (ticketed 20260713130000):** every 30s sweep of every job fails with
  "stored JOB body did not parse: lexing failed: UNEXPECTED_CHAR" — the `/server/jobs` row stores
  the plan as AST JSON while `fire_due` lexes it as statement text. Reproduced identically for a
  config-boot job AND a job installed through the daemon's own commit bridge, so **the sweeper has
  never fired a real job**; the PR #35 E2E passed against a hand-shaped row.
- **What DID pass live:** boot registration (policies=1 jobs=1), the 30s real-clock sweeper with
  at-least-once re-fire, the read-only `/server/jobs/<name>/runs` ledger recording every attempt
  (5 failed runs at exact 30s intervals read back), the **outside read-back path**
  (`POST /api/run {statement: "/server/jobs/heartbeat/runs |> order by scheduled_at DESC", mode:
  "read"}` on the loopback HTTP face), and the commit bridge's `api`-policy default-deny →
  explicit `ALLOW upsert on 'server/*'` grant flow (concern-30 behavior observed exactly as
  documented).
- **Acceptance NOT ticked:** the mission's "Server scheduling semantics designed and implemented"
  stays open until the format-mismatch ticket lands and a re-run records a `fired` outcome with
  the tick file on disk.
- **Residue:** none (no tick file was ever written; serve stopped; state confined to the session
  scratchpad).
