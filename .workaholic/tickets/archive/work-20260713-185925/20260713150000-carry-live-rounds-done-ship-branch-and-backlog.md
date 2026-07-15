---
created_at: 2026-07-13T15:00:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain, Infrastructure]
effort: 0.1h
commit_hash: 27895cd
category: Changed
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# CARRY: live rounds 9/10 done — ship the knowledge branch, then work the 8-ticket defect backlog

## Position (2026-07-13, end of the owner-attended live-round session)

Branch **`work-20260712-114152`**, working tree **CLEAN**, **12 commits ahead of `origin/main`,
all pushed**. **No PR, no story, no release note yet.** Every commit on this branch is
`.workaholic/` knowledge only — live-round evidence appended to archived tickets, mission
checkbox/changelog updates, and 8 new bugfix tickets. **Zero code/binary change**, so
`packages/qfs/crates/qfs/Cargo.toml` is still `0.0.59` (already shipped + tagged as its own
release; PR #35 merged as `c30fa0a`).

The branch began as post-`v0.0.59`-ship housekeeping (release note + PR-35 deferred concerns) and
grew into the whole live-round campaign.

### Live rounds: 9 of 10 done (evidence on each archived ticket; see resume ticket
`20260712050000-resume-owner-attended-live-rounds.md` for the full table)

- **DONE:** 1 Chatwork read, 2 T8 switch re-run (multi-row Drive), 3 Slack user-token post,
  4 Gmail reply-with-Drive-attachment, 5 PDF→Drive, 6 transform chain, 7 all three model
  providers, 8 declared `/ghdecl` GitHub read, 10 Chatwork file upload+download.
- **BLOCKED:** round 9 (`qfs serve` firing a JOB) — the sweeper has a real defect (ticket below);
  re-run it once that fix ships.

### Mission acceptance: 7 of 13 items ticked

Open items (`mission.md`): Slack-bytes attach/detach (#line60), Gmail→Drive transfer (#line67),
switch predicate (#line87), dependency reduction (#line92), server scheduling (#line102),
command-execution-risk assurance (#line104).

## Next actions (in order)

### 1. Ship this knowledge branch (report → PR → ship)

Run `/report` then `/ship` for `work-20260712-114152`. **Decision flag for the owner:** the branch
is **docs/knowledge only — no binary change**, so the CLAUDE.md "bump the patch on every shipped
PR" rule has nothing to release. Options: (a) bump to `0.0.60` + tag anyway to honor the rule
(cuts an identical-binary release), or (b) treat knowledge-only branches as exempt and merge
without a tag. Prior pattern was "housekeeping rides the next code PR," but this branch is now
large and standalone. **Ask the owner which** at ship time; do not auto-bump.

### 2. Work the 8-ticket defect backlog the live rounds surfaced (a fresh `/drive`, new branch after ship)

Prioritized by severity (the live rounds found silent wrong-answer and wrong-node-write paths the
hermetic suite structurally could not):

- **HIGH — `20260713130000-sweeper-job-body-format-mismatch.md`**: the v0.0.59 headline feature
  (daemon fires `CREATE JOB`) has **never fired a real job** — `/server/jobs` stores the plan as
  AST-JSON while `fire_due` lexes it as statement text (`UNEXPECTED_CHAR` every sweep). The PR #35
  E2E passed on a hand-shaped row. Fix both install paths + re-point the E2E, then **re-run live
  round 9** to tick server-scheduling acceptance.
- **HIGH — `20260713120100-drive-update-folder-where-dropped.md`**: Drive `UPDATE` on a folder
  path silently drops `WHERE` and mutates the folder node itself (observed live: renamed the test
  folder). Data-safety severity.
- **MEDIUM — `20260713101500-slack-users-where-silently-dropped.md`**: `/slack/.../users` drops the
  `WHERE` stage (returns all rows); cookbook-taught filter returns wrong rows.
- **MEDIUM — `20260713120000-blob-node-plan-schema-omits-content.md`**: single-file blob nodes omit
  `content` from the plan schema, so the taught PDF-extraction recipe refuses; round 5 needed a
  struct bypass.
- **MEDIUM — `20260713123000-gmail-read-not-serviced-pure-transform-commit.md`**: a read-terminal
  transform chain over `/mail/inbox` fails at commit ("READ is not serviced by the Gmail driver")
  though the switch commit path reads Gmail fine.
- **MEDIUM — `20260713140000-transform-provider-params-reasoning-models.md`**: OpenAI `max_tokens`
  is rejected by reasoning models (HTTP 400); Gemini reasoning models exhaust the `low` token
  budget on thinking → misleading schema-mismatch error. Map `effort` onto per-provider reasoning
  controls.
- **LOWER — `20260713120200-drive-id-addressing-and-space-names.md`**: documented `/drive/id:<id>`
  addressing is `invalid_path`; space-named files are wholly unaddressable.
- **LOWER — `20260712150000-drive-folder-upsert-per-row-parity.md`**: Drive folder `UPSERT` lacks
  INSERT's per-row decode, so the INSERT-collision error's advice is a dead end.

### 3. Close the remaining mission acceptance (later drives / live rounds)

- **Switch predicate (#line87)**: likely already satisfied by round 2 (the T8 switch re-run
  passed live). Verify the evidence and **tick it** rather than re-running — the round-2 evidence
  lives on `20260712005000-drive-multi-row-insert-silent-loss.md`.
- **Slack-bytes attach/detach (#line60)** and **Gmail→Drive transfer (#line67)**: small owner-
  attended live rounds; `files:write` is already consented on the `slack me` mount.
- **Dependency reduction (#line92)** and **command-execution-risk assurance (#line104)**: recorded-
  assessment tickets (`20260711121533`, `20260711121536`), not live rounds — a `/drive` desk task.
- **Server scheduling (#line102)**: unblocks after the sweeper fix (item 2 HIGH) → re-run round 9.

## Operating pattern (proven this session; unchanged)

Statement files in the session scratchpad; assistant runs PREVIEW (model-free) + read-back.
**Owner's standing approval rule (saved to memory `live-writes-self-only-standing-approval`):**
self-visible-only live writes (self-addressed mail, self-DMs, own-Drive, my-chat) are always
approved — proceed without asking; anything other people can see/receive needs an explicit ask.
Keys ride `.env` at repo root (`ANTHROPIC_API_KEY`, `OPENAI_AI_KEY`, `GEMINI_API_KEY`; gitignored)
— source it with `set -a; . .env; set +a`. Vault unlock (`qfs auth`, 8h) is shared. The
assistant's own shell is still denied outward-visible live cloud writes by the classifier, so those
route through the owner's `!` relay after an explicit approve.

## Live-round residue (owner's call to clean)

Drive folders `qfs-switch-test` (2 routed files) and `qfs-extract-test` (1 extracted file, a
space-named file currently unaddressable via qfs — see ticket 20260713120200); one self-addressed
Gmail thread (2 messages + PDF); Slack self-DM (2 test messages, one bot- one user-identity); one
76-byte test file in the owner's Chatwork マイチャット; transform `triage` (pre-existing; all this
session's probe transforms were removed). The GitHub account `gh` and the `/ghdecl` mount were
added this session (keep — round 8 proof).

## Working binary note

The locally-installed `qfs` was updated to **v0.0.59** (`install.sh`) this session — matches the
branch. `-f <file>` is NOT a `qfs run` flag; pipe statements via `qfs run - < file.qfs`.

## Final Report

Archived as a **completed carry** — both its next-actions are done: item 1 (ship the knowledge
branch) merged as PR #36, and item 2 (drive the 8-ticket defect backlog) shipped as v0.0.60 / PR
#37. No implementation was performed by this drive; the ticket is retired to keep the queue clean
per its successor (`20260713181420`) §5. The remaining mission-acceptance live rounds it enumerated
are carried on that successor.
