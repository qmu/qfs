---
created_at: 2026-07-13T12:02:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Documented `/drive/id:<file-id>` addressing is invalid_path; space-named files are unaddressable

## Problem (found live, round 5, v0.0.59)

A Drive file whose name contains spaces (`Fundamental LLM model comparison.pdf`, created by a
transform's model-chosen name) cannot be addressed as a single node at all:

- `/drive/my/qfs-extract-test/Fundamental LLM model comparison.pdf |> select size` →
  `parse_error UNEXPECTED_TOKEN` (path segments cannot carry spaces)
- The gdrive cookbook's documented escape hatch — "address the one you mean by its id
  (`/drive/id:<file-id>`)" — returns `invalid_path` for both spellings tried live:
  `/drive/id:1xVt…` and `/drive/my/id:1xVt…`
- Collection+WHERE forms don't rescue it: UPDATE mutates the folder (ticket 20260713120100)

Net effect: such a file can be listed but not read, renamed, copied, or trashed through qfs —
unreachable content, and models routinely emit space-bearing names.

## Fix

Implement (or repair) the documented `id:` single-node addressing on the gdrive driver — it also
resolves the name-ambiguity case the cookbook cites — and/or give path segments a quoting/escape
form for spaces. Regenerate docs if the surface changes; add a hermetic lock reading a
space-named file by id.

## Key files

- `packages/qfs/crates/driver-gdrive/` — path resolution (name walk; missing id: branch)
- `docs/cookbook/gdrive.md` — documents the id: form today
- parser path-segment token grammar if quoting is chosen

## Acceptance

- A space-named Drive file is readable (content), renamable, and trashable via `id:` addressing.
- The cookbook `id:` claim is true (or rewritten to the real form) with a hermetic lock.

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause: `DrivePath::parse_str` handled the `id:` selector only when the RAW string started with
`id:` (the mount-relative `id:<id>` form the tests used), but an operator types the mounted
`/drive/id:<id>` the cookbook documents. That path starts with `/drive/`, so after stripping the
mount the remainder `id:<id>` fell through to the corpus match and was rejected as
"the /drive root has only the `my` and `shared` corpora" → `invalid_path`.

Fix: `parse_str` now also recognises `id:` AFTER stripping the `/drive/` mount, so
`/drive/id:<file-id>` (and `/drive/id:<file-id>@rev`) parse to the same `DrivePath::ById` as the
bare form (shared `id_selector` helper). The read path (`client.get_file` → content) and the effect
decoders (`decode_move`/`decode_remove` ById arms) already handled `ById`, so read/rename/trash all
work once the address parses. This resolves the space-named-file case too: address it by id, since a
space-bearing name cannot be a path segment.

Scope note: chose the ticket's "implement/repair `id:` addressing" over "give path segments a
quoting/escape form for spaces" — id addressing is the documented, unambiguous single-node form and
resolves the name-ambiguity case the cookbook already cites, with no grammar change.

Cookbook: `docs/cookbook/gdrive.md` already documents `/drive/id:<file-id>` — the claim is now TRUE,
no rewrite needed.

New hermetic locks: `paths_parse_to_corpora_drives_ids_and_revisions` extended with the mounted
`/drive/id:<id>` + `@rev` cases; `a_space_named_file_is_readable_by_the_mounted_id` reads a
space-named PDF's bytes through `/drive/id:<id>`.
