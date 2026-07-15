---
created_at: 2026-07-13T18:14:20+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# RESUME: post-v0.0.60 ship — owner-attended live rounds, code follow-ups, and mission close-out

## Position (2026-07-13, end of the drive→report→ship session)

Working tree **CLEAN** on `main` (= `origin/main`). **Nothing in flight — everything this session
started is shipped.** This ticket is a fresh resumption checkpoint, not carried work.

- **v0.0.60 is released.** PR #37 (the 8 live-round defect fixes) merged to `main`
  (`ac58b41`), tagged `v0.0.60`, and the GitHub Release published with the four native tarballs
  (linux-musl ×2 + darwin ×2 + sha256s) — it is `Latest`. Crate `0.0.59 → 0.0.60`,
  plugin `0.11.2 → 0.11.3`.
- The knowledge branch (PR #36, live-round campaign 9/10) merged earlier this session (no tag —
  knowledge-only, owner-approved exempt).
- All eight defect tickets are archived under
  `.workaholic/tickets/archive/work-20260713-150833/` with per-ticket **Resolution** sections; each
  fix has a hermetic lock (several verified by a negative test). All seven CLAUDE.md gates were
  green at ship.

## Next actions (prioritized)

### 1. HIGH — re-run **live round 9** now that the sweeper fix shipped (owner-attended)

The v0.0.60 sweeper fix (`fire_one` rehydrates the canonical `PlanSpec`) is hermetically proven but
the mission's **server-scheduling acceptance (`mission.md` line 102)** needs the live proof: on
`qfs serve`, a `create job … every '1m' do upsert into /local/... policy heartbeat` fires within
90s — the `/server/jobs/<name>/runs` ledger shows `fired` / `affected 1`, the tick file exists, and
history survives restart. Record evidence on the archived sweeper ticket
(`20260713130000-sweeper-job-body-format-mismatch.md`) and tick line 102. Concern
`37-new-server-scheduling-live-acceptance.md` tracks this.

### 2. Remaining mission acceptance — owner-attended live rounds

- **Switch predicate (line 87)** — likely ALREADY satisfied by round 2 (the T8 switch re-run passed
  live). **Verify-and-tick** rather than re-run: evidence is on
  `archive/work-20260712-015928/20260712005000-drive-multi-row-insert-silent-loss.md`.
- **Slack-bytes attach/detach (line 60)** — small owner-attended round; `files:write` is already
  consented on the `slack me` mount.
- **Gmail → Drive transfer (line 67)** — one qfs statement moving an attachment's bytes from
  `/mail/<msg>/<att>` into a `/drive/<folder>`; owner-attended.

### 3. Remaining mission acceptance — desk tasks (no live)

- **Dependency reduction assessment (line 92)** — recorded adopt-with-plan / defer-with-reasoning
  ruling (ticket `20260711121533`).
- **Command-execution-risk assurance (line 104)** — tests or a governance lock showing no path from
  query text / fetched data to process execution (ticket `20260711121536`).

### 4. Code follow-ups surfaced by the defect fixes (non-live; driveable anytime)

Filed as concerns this session:
- `37-new-drive-select-content-schema-divergence.md` — unify gdrive file-content vs folder-listing
  describe schemas so `/drive/<file> |> select content` type-checks (the `/local` fix did this for
  local; gdrive needs the schema unification first).
- `37-new-driver-fs-content-omission.md` — apply the same `content`-column widen to `driver-fs`
  (`QFS_FS_<NAME>` named roots), which shares the pre-v0.0.60 `/local` omission.
- `37-new-drive-folder-rename-predicate-channel.md` — to rename the matching child (instead of the
  v0.0.60 safe refusal) a same-column `SET name WHERE name` needs a predicate/selector channel
  distinct from the SET row payload.

### 5. Housekeeping — archive two executed session-state tickets

Both are DONE (leave them for the next `/drive` to archive, or archive on sight):
- `20260712050000-resume-owner-attended-live-rounds.md` — the campaign resume; rounds 9/10 status is
  captured above (9 done, round 9 pending the live re-run in action 1).
- `20260713150000-carry-live-rounds-done-ship-branch-and-backlog.md` — its item 1 (ship the
  knowledge branch) and item 2 (drive the 8-ticket backlog) are both complete this session.

## Operating pattern (unchanged, proven)

Statement files in the session scratchpad; assistant runs PREVIEW (model-free) + read-back.
**Owner's standing approval:** self-visible-only live writes (self-addressed mail, self-DMs,
own-Drive, my-chat) are pre-approved — proceed; anything others can see/receive needs an explicit
ask (memory `live-writes-self-only-standing-approval`). Model/API keys ride repo-root `.env`
(`ANTHROPIC_API_KEY`, `OPENAI_AI_KEY`, `GEMINI_API_KEY`; gitignored) — `set -a; . .env; set +a`.
Vault unlock (`qfs auth`, 8h) is shared. The assistant's own shell is denied outward-visible live
cloud writes by the classifier, so those route through the owner's `!` relay after an explicit
approve. `-f <file>` is NOT a `qfs run` flag — pipe via `qfs run - < file.qfs`.

## Working binary note

Re-install the local `qfs` to **v0.0.60** (`install.sh`) before the live rounds so the sweeper fix
and the other seven fixes are present.

## Closed (2026-07-14, branch work-20260714-013531)

Every "Next action" in this checkpoint is now resolved — archiving so the next `/drive` does not
re-surface stale items:

- **Action 1 (re-run live round 9)** and **Action 2 (switch / Slack-bytes / Gmail→Drive live
  rounds)** — all owner-attended and **live-proven**; the mission acceptance boxes for
  server-scheduling (round 9, v0.0.61), switch predicate, Slack-bytes attach/detach, and
  Gmail→Drive transfer are all `[x]`.
- **Action 3 (desk tasks)** — closed on this branch: dependency-reduction ruling recorded
  (blueprint §11) and command-execution-risk assurance verified already-landed (§17); both mission
  boxes now `[x]`.
- **Action 4 (code follow-ups)** — the two schema-widen concerns
  (`37-new-drive-select-content-schema-divergence`, `37-new-driver-fs-content-omission`) were
  resolved on branch `work-20260713-185925`. The third
  (`37-new-drive-folder-rename-predicate-channel` = `per-row-drive-folder-rename-needs`) remains
  the **only open follow-up**, captured as the design-brief ticket
  `20260713195008-effect-selector-channel-folder-rename.md` (stays queued, needs the brief first).
- **Action 5 (housekeeping)** — both executed session-state tickets were archived under
  `work-20260713-185925/`.

**Sole remaining queued item:** ticket `20260713195008` (effect selector/predicate channel —
design brief first, do not ad-hoc `/drive`).
