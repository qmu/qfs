---
created_at: 2026-07-12T15:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Drive folder UPSERT lacks the per-row named decode INSERT has (error advice is a dead end)

## Problem (found live, round 2 of the owner-attended rounds, v0.0.59)

`insert into /drive/<folder>` with rows carrying `name` + bytes decodes one upload per row
(PR #34). When a name collides, INSERT refuses with:

> a Drive file already exists there (id …); INSERT never replaces — **use UPSERT to replace its
> content deliberately**

But following that advice with the same row shape —
`… |> upsert into /drive/my/qfs-switch-test` — refuses with:

> malformed UPSERT effect at "/drive/my/qfs-switch-test": the path names a folder — bytes cannot
> replace a folder

The UPSERT applier takes the single-blob path (folder target + bytes → replace the folder node)
instead of the per-row named decode, so the error's own suggested recovery cannot work for the
folder-INSERT shape. (PR #34's release note says "Multi-row Drive INSERT/UPSERT decodes one upload
per row" — the UPSERT half of that is not true for a folder target.)

## Fix

Give the Drive UPSERT applier the same folder-target per-row decode INSERT has (rows with `name` +
bytes → upsert each `/folder/<name>`), replacing when the name exists and creating when it
doesn't. Alternatively, if folder-target UPSERT is deliberately unsupported, make the INSERT
collision message advise something that works (e.g. upsert to the full file path) — the message
and the surface must agree.

## Key files

- `packages/qfs/crates/driver-gdrive/` — the PR #34 per-row effect decode
  (`from_node_rows_with`), batch applier, and where the UPSERT folder-shape refusal fires
- `packages/qfs/crates/driver-gdrive/` hermetic tests — PR #34 added the live-round 2-row INSERT
  shape verbatim; add the UPSERT twin

## Acceptance

- Hermetic: multi-row UPSERT into a folder path (rows with `name` + bytes) writes/replaces one
  file per row, `affected` = rows written; mixed create+replace batch covered.
- The INSERT collision error's advice, followed verbatim, succeeds.

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause: `decode_upsert` had no per-row named-folder branch. After the `file_id` shortcut it went
straight to `res.existing(path)`; for a folder target that resolved the FOLDER node and refused
"bytes cannot replace a folder". INSERT's `decode_insert` instead routes a row carrying a `name`
through `upload_destination` (the folder becomes the parent, `name` the child), which is why the
multi-row folder INSERT worked but its UPSERT twin did not — contradicting PR #34's release note and
making the INSERT-collision advice ("use UPSERT to replace its content") a dead end.

Fix: `decode_upsert` now, when a row carries a `name` column, resolves `<folder>/<name>` via the
same `upload_destination` INSERT uses, then probes with `child_id`: an existing child → `Update`
(replace content by id); a free name → `build_upload` (create). The single-blob path (folder target
with NO `name`) still refuses, which is correct — that shape genuinely asks to overwrite the folder.
The multi-row fan-out (`from_node_rows_with`) already decodes Upsert per row, so this makes each row
write independently.

New hermetic lock: `multi_row_folder_upsert_replaces_existing_and_creates_new_per_row` — a 2-row
UPSERT into a folder decodes to one `Update` (the colliding `exists.txt` → replace by id) and one
`Upload` (the free `fresh.txt` → create), covering the mixed create+replace batch. INSERT's
per-effect count is already `affected`-honest through the shared batch applier.
