---
created_at: 2026-07-06T12:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 2h
commit_hash: 85141ef
category: Added
depends_on: []
---

# /git @<ref> blob/tree reads: resolve nested subtree paths

## What's wanted

`/git @<ref>` blob and tree reads currently resolve the flat top-level tree (E0) only; a nested
path like `src/foo/bar.rs` fails `invalid_path` because the walker scans one flat tree's entries
for an entry literally named the whole path. Extend the read-side walker to descend nested
subtrees so any committed path resolves as of a ref.

## Current state (verified against HEAD 61f696c)

- `crates/driver-git/src/blobfs.rs::walk_to_tree` (~160-176) and `resolve_blob` (~179-190) match
  `entry.name == path` against a single tree's entries — no `/`-split, no recursive descent. Doc
  comment (line 159): "Walk a (flat, at E0) tree".
- `crates/driver-git/src/lib.rs:49-50` lists "Nested trees" as a named park (read + write).
- The object-model primitives already exist: `objectdb.rs` (`parse_tree`, `TreeEntry::is_tree`,
  `repo.db().read(&oid)`). No new object-store capability is needed — this is a path-walking
  change confined to `blobfs.rs`.

## Implementation steps

1. In `walk_to_tree`, split `path` on `/`; for each intermediate segment find the matching tree
   entry, read+parse the subtree via `repo.db().read`, and descend. Fail-closed with the existing
   structured `invalid_path` for a genuinely missing segment.
2. `resolve_blob` resolves the final segment against the descended tree.
3. Add hermetic tests with a nested-tree fixture (read a blob at `a/b/c`; `ls` a subtree).
4. Update the `lib.rs` named-park note to reflect that read-side nested resolution now ships.

## Key files

- `crates/driver-git/src/blobfs.rs` (`walk_to_tree`, `resolve_blob`), `crates/driver-git/src/objectdb.rs`,
  `crates/driver-git/src/lib.rs` (named-park doc).

## Considerations

- Read-only scope. The write-side flat-tree limitation in `planner.rs` (lines 158, 531) is a
  separate named park — do not conflate.
- Source concern: `.workaholic/concerns/11-git-ref-tree-blob-reads-and.md`.
