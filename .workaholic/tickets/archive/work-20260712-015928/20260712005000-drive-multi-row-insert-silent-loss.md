---
created_at: 2026-07-12T00:50:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Drive folder INSERT loses rows past the first (affected N, one file written)

## Overview

Found live in the owner-attended §18 switch routing round (2026-07-11). A multi-row
`INSERT INTO /drive/my/<folder>` — here the switch 'file' arm's routed partition of **2 rows**
(`name`, `bytes` columns) — reported `affected 2` in the committed summary but wrote **exactly one
file** to Drive (the first row). The second row vanished silently: no error, no partial-failure
report, an overcounted `affected`. Honest-count doctrine violation on the write side.

Two adjacent findings from the same round, same driver:

1. **Missing destination folder fails at COMMIT, not PREVIEW.** `insert into
   /drive/my/qfs-switch-test` with the folder absent previewed clean and failed at apply with
   `Terminal { reason: "could not resolve \"qfs-switch-test\" … no child of this name" }`. The
   gdrive cookbook explicitly promises "a destination that isn't a writable folder (a file path, a
   missing folder) fails with a structured error at preview" — either make the plan-time folder
   resolution real (describe the parent at eval, like the SQL catalog check) or fix the cookbook
   sentence to match reality.
2. Text → bytes coercion on the `bytes` column WORKS (the routed subject text landed as file
   content, 84 bytes) — worth a hermetic lock so it stays true.

## Repro

```sh
qfs run "insert into /drive/my/<existing-folder> values (name, bytes) ('a.txt','one') ('b.txt','two')" --commit
# summary: affected 2; Drive shows only a.txt
```

Or any pipeline write whose source produces >1 row into a `/drive` folder.

## Key Files

- `packages/qfs/crates/driver-gdrive/src/` - the upload apply path (find where the applier consumes `args.rows` — suspect it uploads `rows[0]` only)
- `packages/qfs/crates/exec/tests/oneshot.rs` - `switch_e2e` harness shape for a hermetic multi-row-write lock (capturing applier)
- `docs/cookbook/gdrive.md` - the preview-fails-on-missing-folder promise (finding 1)

## Implementation Steps

1. Locate the Drive write applier's row loop; make it upload every row (one file per row), with
   per-row failure surfacing (a mid-batch failure must not report full success).
2. `AppliedEffect.affected` must equal files actually created.
3. Hermetic test: multi-row folder insert → applier receives/creates N files; affected == N.
4. Rule finding 1: plan-time parent-folder check at eval (preferred) or cookbook correction.
5. Re-run the live switch round (the statement file is in the T8 archive ticket) to confirm 2
   files land.

## Quality Gate

- Hermetic multi-row Drive write test green; `affected` equals real writes.
- Owner-attended re-run writes both routed files.

## Live Round Evidence

### Round 2 — T8 switch re-run, multi-row Drive proof (2026-07-12, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a) from the published release. Statement: the archived T8
  `switch-live-round.qfs` verbatim (inbox top 3 → `transform triage` on
  anthropic/claude-haiku-4-5/effort-low → switch `route` to Drive-file or self-draft arms).
- **Pre-step (owner-approved):** the T8 residue file in `/drive/my/qfs-switch-test` was trashed
  (REMOVE with a name filter, `--commit-irreversible`) so the create-only INSERT arm could re-run
  verbatim; folder read back empty before the round.
- **Committed result:** `#0 CALL transform.triage [affected 3] (!) / #2 INSERT drive [affected 2]
  / #4 INSERT mail drafts [affected 1]` — the model routed the two alert-looking subjects
  (a suspension notice, a CI build failure) to the file arm, one to the drafts arm.
- **The proof:** read-back listed **exactly 2 files, both with this round's timestamps** — affected
  2 = 2 files actually written. The T8 defect (affected 2, one file written) is confirmed fixed
  against real Drive.
- **Bonus findings, both correct-behavior proofs from a first failed attempt:** (1) with the
  residue file still present, the INSERT arm refused with "a Drive file already exists there;
  INSERT never replaces" — create-only semantics held live; (2) that failure **fail-stopped the
  whole plan** — the drafts arm reported `skipped (dependency NodeId(2) failed)`, proving the
  PR #33 dependency-bridge fix live (T8 had observed a stray draft from exactly this case).
- **New defect found (ticketed):** the INSERT refusal advises "use UPSERT to replace its content
  deliberately", but `upsert into /drive/my/qfs-switch-test` (folder path, rows carrying `name` +
  bytes) refuses with "the path names a folder — bytes cannot replace a folder" — the per-row
  named decode exists for INSERT but not UPSERT, so the error's advice is a dead end. See todo
  ticket 20260712150000-drive-folder-upsert-per-row-parity.
- **Residue:** `/drive/my/qfs-switch-test` now holds the 2 routed files; 1 new self-addressed
  draft in Gmail (plus the two from T8).
