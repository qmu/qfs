---
created_at: 2026-07-11T18:54:47+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# RESUME: capability-tryout branch — /report → /ship v0.0.56, then remaining live rounds

## Position (verified 2026-07-12, post switch-live-round session)

Branch **`work-20260711-121525`**, open **PR #33** (base `main`), working tree clean except this
ticket (untracked). Version **`0.0.56`**, plugin **`0.11.0`** (MINOR — the switch stage is a
taught surface). All gates green at handoff: `cargo test --workspace` (1380+ tests), `clippy -D
warnings`, `fmt --check`, `gen-docs --check`, `gen-skills --check`, `check-migrations`.

**Everything implemented on this branch is COMMITTED — do NOT re-implement.** On top of the nine
capability-tryout commits (T1–T7, T9, design-brief-era work; see git log `main..HEAD`):

| commit | what |
| ------ | ---- |
| `10b00d9` | **T8 switch routing implemented** (blueprint §18): `PipeOp::Switch` grammar/AST (contextual `switch`/`else`, bounded arm scan), variant lock 19→20, eval arm-union planning, resolve arm gates, exec commit routing (materialize once → partition → embed/prune → apply), 22 hermetic tests, docs/cookbook/skills, plugin 0.10.7→0.11.0, qfs 0.0.55→0.0.56. Ticket archived: `archive/work-20260711-121525/20260711121532-switch-predicate-model-routing.md` |
| `a59f914` | **Fixes from the owner-attended switch LIVE round** (which RAN and PASSED — full record on the archived T8 ticket): (1) driver-http ambient-runtime panic fix — was silently blocking EVERY live `|> transform` commit; (2) switch prune now BRIDGES dependency edges (declaration order + fail-stop restored); (3) System-DB v16 comment-only in-place edit registered in `SUPERSEDED_BODIES` (dev DBs self-heal); + 2 new todo tickets |

## Immediate priority (FIRST, before more implementation)

1. **`/report`** — PR #33's story/release note predate 11 commits. Watch the known `/ship`
   concern-extract duplicate issue (delete `<pr>-carried-*` files after; see memory).
2. **`/ship`** — merge PR #33, tag **`v0.0.56`** (NOT v0.0.55 — the tag must match
   `crates/qfs/Cargo.toml`), push; release workflow builds the four tarballs.

## Queued implementation tickets (a later /drive, fresh branch after ship)

- `20260712005000-drive-multi-row-insert-silent-loss.md` — Drive folder INSERT writes only the
  first row while reporting `affected N` (live-round finding; includes the
  missing-folder-fails-at-commit-not-preview cookbook mismatch).
- `20260712005100-chatwork-declared-live-read-empty-columns.md` — Chatwork declared driver live
  read returns rows with zero columns (right row count, values lost after decode).

## Remaining owner-attended live rounds (T8's is DONE and recorded)

The `a59f914` http fix UNBLOCKED these (they all commit through the previously-panicking path):

- Slack: post as a real user token to a live workspace (T1). Note: `/slack/<ws>` workspace
  listing is not a readable node (`slack_invalid_path`) — you need real `<ws>/<channel>` names.
- Gmail: a real cross-service reply into a self-addressed thread carrying a Drive file (T3).
- GitHub: a real read through the declared `/ghdecl` driver via `AUTH ACCOUNT 'github'` — the
  live GitHub API needs a `User-Agent` header from the app layer (T4).
- Daemon: a real `qfs serve` firing a harmless 1-minute JOB with run-history read-back (T5).
- Transform: a real PDF × provider key × Drive write, read back (T6); a two-real-stage chain (T7).

**Live-round operating pattern** (worked example = the T8 round, archived ticket): statement
files in the session scratchpad; the assistant runs PREVIEW (model-free) and read-back
verification; the OWNER triggers every COMMIT from a **real terminal** (the `!` relay has no
TTY/stdin; the assistant's shell is denied live cloud writes). Model key: `secret
'env:ANTHROPIC_API_KEY'` + `read -rs` in the owner's terminal — `qfs account add anthropic` is
refused (not a service provider). Vault unlock (`qfs auth`, 8h, disk-backed) is shared. A
`triage` transform definition (anthropic / claude-haiku-4-5-20251001 / effort low /
env:ANTHROPIC_API_KEY) is already installed and reusable.

## Live-round residue (owner's call to keep or clean)

Drive folder `qfs-switch-test` (1 routed file), 2 self-addressed drafts ("Routed by the qfs
switch live round."), the `triage` transform definition (`remove transform triage` drops it).

## Recorded follow-up gaps (unchanged from the earlier drive)

- Slack threaded file-reply (`thread_ts`) and Chatwork file upload (needs a generic `ENCODE
  multipart` primitive) — documented in `docs/cookbook/cross-service.md`.
- OAuth `AUTH ACCOUNT 'google'` refresh bridge — the Google refreshing `TokenSource` needs the
  mount's OAuth `app` plumbed into `declared_secrets` (`crates/qfs/src/declared_driver.rs`).
- Cron daemon: `TokioHost::schedule_jobs` records definitions only; the real `tokio::time`
  interval driving `qfs_watchtower::cron::fire_due` + `/server/jobs/<name>/runs` read-back are
  unwired (blueprint §10).

## Verify (for whoever resumes)

- `git -C <repo> log main..HEAD --oneline` shows 11 commits, HEAD = `a59f914`.
- `grep '^version' packages/qfs/crates/qfs/Cargo.toml` == `0.0.56`; plugin files == `0.11.0`.
- `gh pr view 33` is OPEN; its story predates the last 11 commits (hence `/report` first).
- The todo queue holds exactly: this ticket + the two `20260712005*` driver tickets.
