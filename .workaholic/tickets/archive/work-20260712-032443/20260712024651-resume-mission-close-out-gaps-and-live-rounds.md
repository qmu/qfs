---
created_at: 2026-07-12T02:46:51+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# RESUME: mission close-out — three implementation gaps, then the owner-attended live rounds

## Position (verified 2026-07-12, post v0.0.58 ship)

`main` at the PR #34 merge + ship housekeeping. **v0.0.56 and v0.0.58 both shipped and
release-confirmed** (4 native tarballs + sha256 each, isDraft false). Version `0.0.58`, plugin
`0.11.1`. All gates green at ship. The todo queue holds exactly this ticket.

**Do NOT re-implement anything below — it is all merged:**

- PR #33 (v0.0.56): the whole capability-tryout implementation arc — 13 tickets including switch
  routing (live-proven), live providers, PDF input, chains, AUTH ACCOUNT, CREATE JOB semantics,
  exec-inventory lock, dependency snapshot, and the a59f914 live-round fixes (driver-http
  ambient-runtime panic; switch prune edge-bridging; System-DB v16 heal).
- PR #34 (v0.0.58): the two switch-live-round driver defects — multi-row Drive INSERT uploads
  every row (`affected` = files created, `partial_apply` on mid-batch failure), and a
  zero-column declared `OF` contract refuses pre-network with newest-wins type lookup so
  re-installing `chatwork.qfs` heals the stale pre-§5.4 type rows (this host's System DB has
  them; the refusal was verified against it with the real binary).

## Why this ticket

The mission's acceptance ("every capability verified live at least once with the real
account/key") still has unticked boxes. What remains falls into two phases: three implementation
gaps that BLOCK acceptance items, then the owner-attended live rounds that tick them. The owner
directed covering ALL of them (2026-07-12).

## Phase 1 — implementation gaps (a /drive on a fresh branch; hermetic-first as usual)

1. **Daemon real-clock sweeper + run read-back** (blocks "server scheduling implemented" + live
   round T5). `TokioHost::schedule_jobs` (`packages/qfs/crates/qfs/src/host.rs`) records
   definitions only; wire the `tokio::time` interval driving `qfs_watchtower::cron::fire_due`
   with the live Committer, durable `last_run`, run-ledger appends, and the
   `/server/jobs/<name>/runs` collection (blueprint §10 records the ruled semantics: missed-fire
   catch-up once, overlap skip, UTC only).
2. **Chatwork declared file transfer: two generic primitives** (blocks "attach/detach over
   Chatwork"). Add `FOLLOW <field>` (a second GET whose host joins the driver's allowed set —
   the cross-host `download_url` follow) and `ENCODE multipart` (upload) to the declared
   evaluator (`packages/qfs/crates/exec/src/declared.rs` + driver-http encode side). NEVER
   Chatwork-specific code; the gaps are recorded in-asset in `chatwork.qfs` with statement
   shapes. Then extend `chatwork.qfs` with the file download/upload views/maps.
3. **GitHub live `User-Agent` header** (blocks the OAuth-style declared live round T4). The live
   GitHub API rejects requests without a User-Agent; supply one from the app layer for declared
   `/ghdecl` reads (`packages/qfs/crates/qfs/src/declared_driver.rs` /
   `crates/driver-http`). Small.

Optional if time allows: newest-wins (or replace-on-install) for driver/view/map rows —
concern `34-new-duplicate-declaration-rows-still-resolve.md`.

## Phase 2 — owner-attended live rounds (one attended session can cover most)

Operating pattern (worked example = the T8 switch round, archived ticket
`archive/work-20260711-121525/20260711121532-switch-predicate-model-routing.md`): statement
files in the session scratchpad; assistant runs PREVIEW (model-free) and read-back verification;
the OWNER triggers every COMMIT from a real terminal (the `!` relay has no TTY; the assistant's
shell is denied live cloud writes). Model key: `secret 'env:ANTHROPIC_API_KEY'` + `read -rs` in
the owner's terminal. Vault unlock (`qfs auth`, 8h) is shared. A `triage` transform definition
(anthropic / claude-haiku-4-5 / effort low) may still be installed and reusable.

| # | Round | Ticks acceptance |
|---|-------|------------------|
| 1 | Re-install `chatwork.qfs` → `/chatwork/rooms` shows `room_id`/`name` values | Chatwork declared (read half) |
| 2 | Re-run the T8 switch statement → BOTH routed files land in Drive, `affected 2` | multi-row fix proof + Drive attach |
| 3 | Slack user-token post (`/slack-me` preview+commit) | Slack file-handling remainder |
| 4 | Gmail reply into a real self-addressed thread carrying a Drive file | Reply-with-attachment |
| 5 | Real PDF × provider key × Drive write, read back | PDF→text→Drive |
| 6 | Two-real-stage transform chain (small models, capped tokens — cost doubles) | Transform chain |
| 7 | OpenAI + Google live text generation (Anthropic already live-proven in T8) | "every major provider" |
| 8 | Declared `/ghdecl` read via `AUTH ACCOUNT 'github'` (after Phase 1 gap 3) | OAuth-style declared e2e |
| 9 | `qfs serve` firing a harmless 1-minute JOB + `/server/jobs/<name>/runs` read-back (after Phase 1 gap 1) | Server scheduling |
| 10 | Chatwork file attach/detach via the new primitives (after Phase 1 gap 2) | Chatwork attach/detach |

Record each round's evidence on the corresponding ARCHIVED ticket (as the T8 round did) and tick
the mission acceptance checkbox in
`.workaholic/missions/qfs-capability-tryout-file-handling-transformation-and-platform-hardening/mission.md`.

## Live-round residue (owner's call, unchanged)

Drive folder `qfs-switch-test` (1 routed file — becomes 2 after round 2), two self-addressed
drafts, the `triage` transform definition (`remove transform triage` drops it).

## Phase 1 — IMPLEMENTED (branch work-20260712-032443, v0.0.59, 2026-07-12)

All three gaps landed with hermetic tests (2432 workspace tests green; fmt/clippy/gen-docs/
gen-skills/check-migrations all green):

1. **Daemon sweeper + runs read-back** — `crates/qfs/src/sweeper.rs`: `LiveCronCommitter`
   (policy gate + IrreversibleGuard + the real `apply_plan`), `sweep_once` (durable `last_run`
   hydration → `fire_due` → run history + stamps + ledger), `spawn_sweeper` (30s `tokio::time`
   interval on the blocking pool, sequential ⇒ structural overlap-skip), wired in `serve.rs`.
   `/server/jobs/<name>/runs` is a READ-ONLY telemetry collection (`job_runs_schema` in core,
   select-only capabilities, capped 50, dies with the job row) — deliberately NOT a `ServerNode`.
   Hermetic tests include a real local-FS fire through the live committer.
2. **FOLLOW + ENCODE multipart** — `PipeOp::Follow` (contextual ident, variant lock 20→21),
   `|> ENCODE <fmt>` between target and VALUES desugars onto `EffectBody::Pipeline`; evaluator
   splits at FOLLOW (second GET via injected closure, NO credential, one-row `content` bytes);
   generic `multipart/form-data` encoder in driver-http (filename convention, deterministic
   boundary); wire paths may carry a `?query=` suffix (lexer query mode). `chatwork.qfs` now
   declares the blob view + multipart upload map (gap comments replaced; asset test 8→10 stmts).
   E2E hermetic: follow download (foreign host, no token leak) + multipart upload (full commit
   stack). Cookbook chatwork/automation updated; plugin 0.11.1 → 0.11.2.
3. **User-Agent** — every declared driver's `rest_config()` carries `User-Agent: qfs/<version>`
   as a default header (live + describe mounts).

Phase 2 (owner-attended live rounds) continues on the follow-up resume ticket.

## Verify (for whoever resumes)

- `git -C <repo> log --oneline -3` on main shows the PR #34 merge + concern commits;
  `gh release view v0.0.58` lists 4 tarballs, isDraft false.
- `grep '^version' packages/qfs/crates/qfs/Cargo.toml` == `0.0.58`; plugin files == `0.11.1`.
- The todo queue holds exactly this ticket; concerns
  `33-new-remaining-owner-attended-live-rounds.md`, `33-new-declared-model-and-scheduling-follow.md`,
  `34-new-duplicate-declaration-rows-still-resolve.md` are the open trail this ticket executes.
