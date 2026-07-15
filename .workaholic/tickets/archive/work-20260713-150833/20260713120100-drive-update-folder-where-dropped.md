---
created_at: 2026-07-13T12:01:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Drive UPDATE on a folder path silently drops WHERE and mutates the folder itself

## Problem (found live, round 5, v0.0.59 — severity: wrong-node write)

Intending to rename a file INSIDE a folder:

```qfs
update /drive/my/qfs-extract-test
  set name = 'extracted-llm-comparison.txt'
  where name == 'Fundamental LLM model comparison.pdf'
```

previewed `affected unknown` and, on commit, **renamed the folder `qfs-extract-test` itself** to
`extracted-llm-comparison.txt` — the WHERE filter was silently ignored and `set name` applied to
the path target node. The file inside was untouched. (Live-observed; the folder was renamed back
by a second path-addressed UPDATE, which is the correct-behavior form.)

This is the third silently-dropped-filter defect the live rounds surfaced (Slack users WHERE,
ticket 20260713101500; Drive folder UPSERT shape, ticket 20260712150000) — but this one MUTATES
the wrong node, so it is the most dangerous: a triage flow doing "rename the matching files"
renames the folder instead.

## Fix

A collection-shaped UPDATE (folder path + WHERE over listing columns) must either apply per
matching row (the Gmail relabel semantics: read to find, act by id) or refuse loudly ("folder
path UPDATE takes no WHERE; address the file node"). Never apply a row-filtered statement to the
container node. Audit REMOVE on the same shape (round 2 used REMOVE + where name == successfully
— confirm it actually filtered rather than trashed-by-path-luck with one file present).

## Key files

- `packages/qfs/crates/driver-gdrive/` — UPDATE effect decode for folder vs file paths
- hermetic locks: folder+WHERE update filters or refuses; file-path update renames the file

## Acceptance

- The statement above renames exactly the matching file (or refuses with a structured error
  naming the file-node form); the folder is never the mutated node.
- A REMOVE with the same shape is locked to per-row semantics too.

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause confirmed as two-layer: (1) `core::eval::setwhere_row_batch` flattens an `UPDATE`'s SET
assignments and WHERE equality keys into one row batch and **de-dups a filter key that shares a SET
column name** — so `SET name … WHERE name == …` loses the WHERE entirely; (2) `driver-gdrive`
`decode_move` then resolved the PATH node (the folder) and renamed it. The design carries no
separate predicate channel (SQL distinguishes WHERE from SET only because they use different
key/non-key columns), so `SET name WHERE name` is fundamentally unrepresentable here.

Fix (chose the ticket's blessed "refuse loudly" option — safe, and the only reliable signal given
the collision): `decode_move` now, for a NAME-path target, resolves the node and **refuses a rename
when it is a folder**, with a structured `malformed_effect` directing the caller to the file node
(to rename a file) or to `/drive/id:<id>` (to rename the folder itself). Unaffected: file renames
by path; folder moves (add/remove parents) by path; and folder rename addressed by id or a
snapshotted `file_id` (the unambiguous forms) — so the "rename the folder itself" capability
remains, just via the explicit address.

- Acceptance 1 met: the live statement now refuses loudly; the folder is never mutated. (Per-row
  rename of the matching child is deferred until a real predicate channel exists — noted below.)
- Acceptance 2 (REMOVE audit): confirmed already correct. REMOVE has no SET, so its WHERE key is
  never de-duped; `remove_target_id` resolves `path.child(name)` for a single `name` filter and
  fails closed on a richer filter (covered by `set_wide_remove_resolves_by_name_or_fails_closed`
  and `path_addressed_remove_resolves_and_trashes`).

**Follow-up (not blocking, noted):** to support per-row folder rename (`SET name WHERE name`)
instead of refusing, the effect representation needs a predicate/selector channel distinct from the
SET payload so a same-column filter survives to the driver. Tracked as a design item, not this
safety fix.

New hermetic locks: `update_set_name_on_a_folder_name_path_refuses_the_wrong_node_write`,
`update_set_name_on_a_file_name_path_renames_the_file`, `update_renames_a_folder_addressed_by_id`.
