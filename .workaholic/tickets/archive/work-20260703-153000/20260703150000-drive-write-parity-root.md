---
created_at: 2026-07-03T15:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 2h
commit_hash: 9a298dd
category: Changed
depends_on: []
---

# Drive writes under the My Drive root fail: upload needs the resolved parent_id

Live parity check vs gdrive-ftp (owner-authorized, 2026-07-03, v0.0.17): every WRITE whose parent
is the My Drive ROOT fails at commit with
`malformed INSERT effect at "/drive/my/...": upload needs the resolved parent_id` — both
`upsert into /drive/my/<file>` (gdrive-ftp `put`) and folder creation. `/drive/my` itself resolves
with SELECT/LS only, so the skill's documented `insert into /drive/my values (name, mime_type)`
mkdir recipe is rejected at resolve, and deeper paths preview fine but die at apply. gdrive-ftp
can put/mkdir at the root.

**The preview lies:** all these forms preview `affected 1` and only fail at commit. In-folder
writes (existing parent, e.g. `/drive/my/設計書/x.txt`) preview fine and are believed to work but
were NOT commit-verified (scope: awaiting an owner-provided test folder).

## Fix

Resolve `parent_id` for root-parent paths (My Drive root has the well-known alias `root` in the
Drive API) in the apply path; make folder-create addressable at any level (decide the statement
shape: INSERT into the parent with (name, mime_type) — then `/drive/my` needs INSERT — or INSERT
at the folder's own path). Align describe/preview with what apply can actually do so the preview
never claims an effect the applier will refuse. Live-verify put/mkdir/cp/mv/trash at root AND in
a subfolder (epic 20260630203020's checklist).

## Key files

- `packages/qfs/crates/driver-gdrive/` (apply/upload path — the parent_id resolution),
  `crates/qfs/src/commit.rs` (drive apply wiring), `docs/cookbook/gdrive.md` (mkdir/upload
  recipes must match what ships)

## Quality Gate

- `upsert into /drive/my/<file>` and a root-level folder create commit successfully live.
- Preview and apply agree: no statement previews `affected 1` and then fails with
  malformed-effect at commit for a reachable parent.
- Cookbook recipes re-verified live; gen-skills regenerated.
