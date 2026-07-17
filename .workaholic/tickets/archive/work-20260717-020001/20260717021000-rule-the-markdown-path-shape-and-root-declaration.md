---
created_at: 2026-07-17T02:10:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: markdown-trees-are-queryable-as-documents-and-links-tables
---

# Rule the markdown path shape and root declaration (design brief)

## Overview

Mission acceptance item 1: fix, in a recorded design ruling, (a) the path shape under which a
scanned markdown tree surfaces, and (b) how a root directory is declared — riding the
declared-drivers convention (mission `declared-drivers-are-the-normal-way-to-add-a-service`):
a committed, reviewable declaration; **no name-shaped `QFS_*` env var as the working path**;
multiple roots on one box must be possible (qfs-viewer targets arbitrary repo checkouts).

Verified against source this session:

- The canonical local-connection mechanism is the **`path_binding` registry** (System DB,
  ledgered writes — re-homed by `20260716143641`). `qfs connect /git/<repo> --driver git
  --at '<path>'` upserts a row `{path, driver_id, at_locator, …}` in one transaction with its
  audit row + `ddl_event` (`connection.rs:462 run_connect`), and the composition roots load it
  (`git.rs:162 path_binding_git_connections`). Cloud mounts and declared REST drivers read
  `path_binding` ONLY; `connections.qfs` and `QFS_SQL_*`/`QFS_GIT_*` are warned, deprecated
  seams a sibling ticket (`20260716214200`) is retiring. A NEW driver must therefore ride
  `path_binding` only — never adding to the deprecated seams.
- `run_connect` accepts any non-cloud driver id without an account
  (`connection.rs:621 require_account_for_cloud_connect` → `is_cloud_driver` false → Ok), so
  `markdown` needs no connect-arm change.

## The ruling (recorded as the design brief in the mission directory)

- **Path shape**: mount `/markdown`; one declared root `<name>` surfaces exactly two tables,
  `/markdown/<name>/documents` and `/markdown/<name>/links` (strategy vocabulary one-to-one).
- **Root declaration**: `qfs connect /markdown/<name> --driver markdown --at '<abs-root-dir>'`
  (CLI) or the language `CONNECT /markdown/<name> TO markdown AT '<root>'` — both land the SAME
  ledgered `path_binding` row. Multiple roots = multiple bindings. No env var ships at all.
- **`source_section_path` encoding**: a JSON array of heading texts (`ColumnType::Array(Text)`),
  top-level first, nearest last; `[]` for a pre-heading link. Array encoding is lossless by
  construction (no delimiter can collide with heading text) and is the documented contract.
- **No typing, by construction**: the `links` schema carries no relation-type column; the closed
  relation vocabulary is a later, separate mission layered on `source_section_path`.
- **Rescan**: slice 1 ships a stateless read-through scanner — every engine scan re-walks and
  re-parses the declared root, so the tables can never go stale and the rescan entry point is
  the scan itself. If a later slice adds an index/cache, an explicit `CALL markdown.rescan`
  becomes the entry point; hot reload / file watching stays unshipped without penalty.

## Implementation Steps

1. Write `path-shape-design-brief.md` in the mission directory
   (`.workaholic/missions/active/markdown-trees-are-queryable-as-documents-and-links-tables/`),
   following the design-brief format (state → ruling → consequences), recording the above with
   source citations.
2. Append the ruling to the mission `## Changelog`.

## Key Files

- `.workaholic/missions/active/markdown-trees-are-queryable-as-documents-and-links-tables/mission.md`
- `.workaholic/missions/archive/language-design-review-layering-principles-and-semantic-gaps/shell-face-design-brief.md`
  — the design-brief format precedent.
- `packages/qfs/crates/qfs/src/connection.rs`, `git.rs`, `path_binding.rs` — the convention the
  ruling rides.

## Policies

- `workaholic:implementation` / `anti-corruption-structure` — one declaration registry, no new
  env-var seam.
- `workaholic:design` — reviewable, committed declarations; secrets referenced never inlined
  (markdown roots carry no secret at all).

## Quality Gate

1. The brief exists in the mission directory and fixes: path shape, declaration mechanism,
   section-path encoding, the no-typing rule, and the rescan ruling.
2. The two implementation tickets (20260717021100, 20260717021200) implement exactly what the
   brief rules — no drift between the brief and the shipped surface.
