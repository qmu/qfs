---
created_at: 2026-07-05T09:52:08+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort: 0.5h
category: Changed
depends_on: []
---

# RESUME: overnight /drive of 2026-07-05 shipped env-hygiene + §13 surface + §13 evaluator FOUNDATION.
# Two owner decisions were made. Continue with a FRESH `/drive` in the order below.

Written at the end of the 2026-07-05 overnight `/drive` (branch `work-20260705-032203`, version
`0.0.22`). The branch is GREEN (2036 tests, clippy/fmt/gen-docs/gen-skills all sync) and carries
real shippable value; it is NOT yet `/report`+`/ship`-ed (owner's call).

## Shipped on the branch this session (5 commits, all green)

1. **env-mutating test hygiene** (20260705022000) — ARCHIVED. Killed the parallel-`cargo test`
   `XDG_CONFIG_HOME` race that failed CI (shared lock + fresh per-test tempdir, non-poisoning,
   cfg(test) `$HOME`-fallback guard). Bumped 0.0.21 → **0.0.22**.
2. **§13 declared-driver SURFACE** (145136) — ARCHIVED. `CREATE DRIVER/TYPE/VIEW/MAP` → `INSERT INTO
   /sys/drivers` (no new Statement variant, zero new keywords) + the `/sys/drivers` storage node
   (migration #14).
3. **§13 evaluator FOUNDATION + host confinement** (145137, PARTIAL — checkpoint commit `66669aa`,
   ticket NOT archived) — the loader, the compiled-wins two-source registry (+report), DESCRIBE
   purity, and the **host-confinement guard** (`allowed_hosts` + the `send_one` chokepoint). Gate
   tests 3 (compiled-wins) + 4 (DESCRIBE zero-network) + 2's read side pass.

## Owner decisions made this session (baked into the tickets)

- **split-brain (20260705000500): canonical mechanism = `qfs connect`** (persisted `path_binding` DB
  binding). The ticket is now UNBLOCKED — implement per its new "DECISION" section (converge the
  runtime + describe registries on `path_binding`; add a `sql` cred-free describe catalog mount;
  env-var/`connections.qfs` become read-only shims; add hermetic `describe /sql/<conn>/<table>` e2e).
- **§13 evaluator remainder: continue AUTONOMOUSLY in the next `/drive`** (owner-endorsed). The
  security boundary's foundation + confinement guard are already landed + tested, so the remainder
  is safe to drive incrementally.

## UPDATE (later same session): the ENTIRE §13 trio SHIPPED

145137 (evaluator, incl. the `/rest` path-impedance fix via `MountRemap::new_prefixed`, live
read/apply wiring, full hermetic read+write e2e, three-layer host confinement) and 145138
(conformance public API + `slack.qfs` twin + 5 recorded parity parks) both landed GREEN and are
**archived**. The whole self-hosting-integrations feature is done.

## THEN — the drivable queue is now EMPTY (all shipped this session)

split-brain (20260705000500) also SHIPPED (owner picked `/sql/<conn>`; `conn_registry`/`has_connections`
+ the `sql` describe mount converge on `path_binding`; convergence e2e green) and is **archived**.
Every drivable ticket in the queue was finished this session — env-hygiene, the whole §13 trio, and
split-brain. What remains is HELD on owner input, not drivable:

- `20260703040000-create-account-language-surface` — BLOCKED on 4 unresolved owner design decisions
  + a queue-external dependency. Needs a design session (`/trip`), not a drive.
- `20260630203090-cf-live-d1-kv-queue` — ICEBOX: needs the owner's CF API token + account id.
- `20260630203000-epic-replace-gmail-ftp-gdrive-ftp` — EPIC/tracking, not an impl ticket.

Next action is the owner's: `/report` + `/ship` this branch (v0.0.22), or open create-account with a
design session. Nothing here is a drive.
3. **split-brain (20260705000500)** — now unblocked (qfs connect canonical); a well-scoped ~4h fix.

## Held (still need owner input — NOT the next agent's job)

- `20260703040000-create-account-language-surface` — BLOCKED on 4 unresolved owner design decisions
  + a queue-external dependency. Needs a design session, not a drive.
- `20260630203090-cf-live-d1-kv-queue` — ICEBOX: needs the owner's CF API token + account id.
- `20260630203000-epic-replace-gmail-ftp-gdrive-ftp` — EPIC/tracking, not an impl ticket.
- `20260705013000-resume-declared-driver-trio-scoping` — the trio scouting note; 145136 done,
  145137/145138 scouting now lives in those tickets. Safe to delete once the trio ships.

## When to ship

The branch (env-hygiene + §13 surface + evaluator foundation) is shippable value at 0.0.22. `/report`
+ `/ship` at the owner's call — no need to ship per ticket; keep accumulating until the evaluator
trio lands, then ship.

## Build-host + safety rules (unchanged — do not relearn)

- `cd packages/qfs`; per Bash call re-export `PATH=$HOME/.cargo/bin:$PATH` +
  `TMPDIR=/home/ec2-user/projects/qfs/.tmp` + `CARGO_INCREMENTAL=0`. `command rm` (rm is
  trash-aliased). **Run gates SEQUENTIALLY** (parallel test+clippy filled the disk to 100% this
  session; `cargo clean` freed 14.8G). Full `cargo test --workspace` ~8-10 min from clean — capture
  to a file, never pipe `fmt --check`/tests through `tail` (masks the exit).
- Gates before every archive: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
  warnings` (NOT `--all-features`), `cargo fmt --all --check`, `gen-docs --check`, `gen-skills
  --check`. Never hand-edit generated docs / a `SKILL.md`.
- Commit/archive ONLY via workaholic `commit.sh` / `archive.sh` (6 message fields THEN files; plain
  words, no backticks/`$()`). Branches only via `skills/branching/scripts/create.sh`.
- Respond in Japanese; own regressions plainly; don't over-ask after a directive.
