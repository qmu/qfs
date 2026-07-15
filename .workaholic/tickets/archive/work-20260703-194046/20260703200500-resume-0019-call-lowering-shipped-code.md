---
created_at: 2026-07-03T20:05:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort:
commit_hash: c377dbf
category: Changed
depends_on: []
---

# RESUME: v0.0.18 SHIPPED; call-effect-lowering done+committed on work-20260703-194046 (v0.0.19, unshipped); parity queue continues

Context checkpoint for a fresh `/drive`. Two things happened this session:

1. **v0.0.18 shipped** (owner said "今 ship"). PR #16 merged to `main` (`4ac555a`), tag `v0.0.18`
   pushed, release published — `gh release view v0.0.18` shows isDraft false + all 8 assets
   (4 tarballs + 4 sha256). Post-merge confirmation recorded on `main` (`78229d5`). The Drive
   write-parity fix is live; `install.sh` can fetch v0.0.18.
2. **call-effect-lowering implemented + LIVE-VERIFIED** on branch **`work-20260703-194046`**
   (TWO commits), **version bumped 0.0.19**, all gates green, ticket archived. **NOT pushed, no
   PR — awaiting the owner's ship decision**, mirroring how v0.0.18 was handed off.

## What the two commits did (drive.copy live-verified; mail.send send owner-attended)

`|> call driver.proc(args)` never lowered to an effect in the one-shot path — so `drive.copy`
returned file rows and copied nothing (mail.send / github.merge / slack.post dropped the same way).
The fix took TWO layers (the first alone was insufficient — a live check proved the CLI still
returned rows):

- **`5faab4a` — eval lowering.** A **terminal** CALL to an **effect** procedure (`ProcSig.returns
  == None`) now evaluates to a single `EffectKind::Call` plan node (target = source path; the
  applier re-resolves the entity live, like `REMOVE … WHERE`; named args → one row keyed by
  name / declared-param; irreversible per `ProcSig`). A CALL to a **result-returning** proc
  (`returns: Some`) still folds to a read. `crates/core/src/eval.rs` (`eval_terminal_call` +
  `call_row_batch`), `crates/core/src/resolve.rs` (`resolve_call_lowering` → new `CallLowering`),
  tests in `eval/tests.rs` + `skill/tests/golden_corpus.rs` (two tests had encoded the buggy
  "CALL is a pure read" behavior — corrected).
- **`0c4aac8` — one-shot routing (the real gap).** `qfs-exec`'s `run_oneshot_inner` branched on
  the parsed **Statement variant** and sent every `Statement::Query` to the read path, so the eval
  Plan was never used. Now a Query whose terminal op is a `CALL` is built first and, if it yields
  effect nodes, routed to the shared `preview_or_commit` effect path; every read without a terminal
  CALL is byte-for-byte unchanged. 4 new regression tests in `crates/exec/tests/oneshot.rs`.
- **LIVE (owner account, this session) — BOTH PATHS PASS:** `drive.copy` previewed as
  `CALL drive.copy`, `--commit` produced a **byte-identical copy (md5 match)**, then trashed —
  full round-trip. `mail.send` (owner-confirmed "実送信OK") previewed as `CALL mail.send`
  **irreversible:true**, `--commit --commit-irreversible` **sent a real email to a@qmu.jp** which
  landed in SENT (id 19f287c95bdfdc4d, subject "qfs live-check: terminal CALL lowering (v0.0.19)").
  The ticket's LIVE quality gate is fully satisfied. All gates: 141 suites / **1941 tests**, clippy
  -D warnings, fmt, gen-docs/skills.

## Remainder for THIS ticket (cookbook, then ship)

- The live-check test email (self-addressed, "Safe to delete") is still in SENT/INBOX — harmless
  evidence; the owner can delete it.
- **Cookbook**: re-add the `drive.copy` recipe to `docs/cookbook/gdrive.md` (removed in v0.0.18 as
  unwired) now that it works, and regenerate gen-skills. NOTE `decode_call` wants `parent_id` as a
  **raw Drive folder id**; for true `cp` parity, resolve a destination **path** to its id in
  `decode_call` (mirror the v0.0.18 apply-time path→id resolution) so the recipe takes a folder
  path, not an opaque id. This is the last piece before v0.0.19 is fully done.
  (Historical safety note — needs the owner's account: `~/.qfs-test-pass`
  vault pattern, `QFS_PASSPHRASE=$(cat …)`, never echo the file; never run setup verbs as agent).
- **Cookbook**: re-add the `drive.copy` recipe to `docs/cookbook/gdrive.md` (removed in v0.0.18 as
  unwired) once live-verified, and regenerate gen-skills. NOTE `decode_call` currently wants
  `parent_id` as a **raw Drive folder id**; for true `cp` parity, resolve a destination **path** to
  its id in `decode_call` (mirror the v0.0.18 apply-time path→id resolution) so the recipe can take
  a folder path, not an opaque id.

## Do in order (fresh /drive)

1. **Owner decision**: push `work-20260703-194046` → `/report` → `/ship` v0.0.19 (deploy contract
   `.workaholic/deployments/github-release.md`: merge, tag v0.0.19, poll `gh release view` for 8
   assets) — OR let the owner test the local build + do the live drive.copy/mail.send verification
   first, THEN finish the cookbook re-add on the same branch before shipping.
2. Then the rest of the parity queue: **20260703150100 mail-drafts-write-parity** (positional draft
   INSERT dies at commit; set-wide `remove /mail/drafts` CapabilityDenied while describe says
   remove:true), **20260703150200 read-projection-fidelity** (user labels list as raw ids;
   draft-attachments read back `[]` — the extraction side is broken), **20260703150300
   agent-facing-doc-gaps**, **20260703150400 plugin-cache-staleness**.
3. Still parked (owner input/tokens): 20260703040000 CREATE ACCOUNT surface, 20260630203090 /cf
   live, epic 20260630203000 (close after the gmail write leg reaches parity).

## Build-host + safety rules (unchanged, do not relearn)

- `cd packages/qfs`; `export PATH=$HOME/.cargo/bin:$PATH` (cargo not on PATH in a fresh shell) +
  `TMPDIR=/home/ec2-user/projects/qfs/.tmp` + `CARGO_INCREMENTAL=0` per cargo run (re-export every
  Bash call); `command rm` (rm is trash-aliased); full suite ~8-10 min — capture to a file, DON'T
  pipe `cargo test` through `tail` (the pipe masks cargo's exit code and truncates the summary).
- Branches only via `skills/branching/scripts/create.sh` (a hook blocks hand-named branches).
  Commit/archive only via workaholic commit.sh / archive.sh (6 message fields, plain words, no
  backticks/$()). `/ship` extraction can duplicate carried concerns — delete `<pr>-carried-*` after.
- Never run qfs setup verbs against the real `~/.config/qfs` as an agent. Respond in Japanese; own
  regressions plainly.
