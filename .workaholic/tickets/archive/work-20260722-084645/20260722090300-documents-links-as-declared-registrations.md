---
created_at: 2026-07-22T09:03:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260722090200-per-row-decode-over-collected-sets.md]
mission: a-file-collection-is-a-declared-set-over-any-blob-source
---

# documents and links become declared registrations, row-equivalent to /markdown

## Overview

Mission acceptance item 5. The markdown knowledge surface (`documents` + `links`) becomes two
**declared, named registrations over the generic collection path** — stored views created
through the existing definition layer (previewed, audited, reconciled by plan/apply like every
other definition) — using the per-row decode and the link relation as the design brief ruled
them.

The bar is **row-equivalence to the compiled `/markdown` driver on the same fixture trees**,
proven by a hermetic equivalence test covering:

- `title` derivation,
- front matter,
- `target_doc` normalization,
- the full nested `section_path` (JSON array, pre-heading `[]`).

The markdown interpretation (ATX headings, inline links, nested section paths) rehomes from
`crates/driver-markdown` into the codec layer as the second named relation — the driver crate's
pure parser is the starting material, moved, not rewritten.

`DESCRIBE` reports both registered views' schemas, so the viewer and agents discover them
generically.

## Policies

- The link graph is a preserved capability: qfs-viewer's backlinks/relations depend on
  `links.section_path`. Losing or flattening it is a regression, not a simplification.
- Registration is an ordinary definition-layer write — no new grammar, no bespoke endpoint.
- The compiled driver is NOT deleted in this ticket; it stays as the equivalence oracle until
  the retirement ticket (the §13 twin-and-retire ratchet aimed inward).
- Hermetic only; qfs-viewer's own repo/package is out of scope.

## Quality Gate

- The hermetic equivalence test passes: declared `documents`/`links` reproduce the compiled
  driver's rows on the shared fixture trees, including the four listed dimensions.
- A `DESCRIBE` test shows both registered views with their schemas.
- `cargo test --workspace`, clippy `-D warnings`, `cargo fmt --all --check`,
  `gen-docs --check` all pass.

## Replan note (2026-07-23) — design settled; complete the registration, do NOT re-escalate

The two points the overnight leaf flagged are already ruled in the mission's
`design-brief-codec-relation-surface.md` (Ruling 3), so no fresh design decision is needed:

1. **Provenance column names.** `documents` uses `path`; `links` keeps the compiled driver's
   `source_doc` / `target_doc`. There is no "source_doc vs path" fork: `links.source_doc`'s VALUE
   is the file's root-relative `path` (the brief asserts both are the same value). `links` carries
   two distinct document references (source and target), so a single `path` column cannot name
   both — the compiled names stay, and row-equivalence + qfs-viewer backlinks require them.
2. **`/local` path normalization.** Root-relative `path` is the join id (brief Ruling 3). The
   `/local` listing carries the VFS path (`/local/notes/a.md`); deriving the root-relative form is
   a small resolution step stripping the declared-collection root prefix — implementation, not a
   design choice.

**Done so far:** the codec half shipped (commit 0a894ef) — `md.documents`/`md.links` named
relations + a hermetic row-equivalence test vs the compiled driver.

**Remaining (drive this ticket to completion):** the declared-VIEW registration through the
definition layer; `DESCRIBE` of the registered views; the `/local` root-relative derivation; and
the registration-level equivalence + `DESCRIBE` tests named in the Quality Gate above. Then the
retirement ticket (20260722090400) deletes the compiled driver once the equivalence gate is green.

## Drive note (2026-07-23) — query surface done; registration layer blocked on disk

Overnight leaf progress (commit `2e23317`, per-crate green):

- **Done, committed:** the `decode <fmt>.<relation>` grammar suffix (`Codec.relation`, parser)
  and its exec wiring — `decode md.documents` / `decode md.links` now parse and route through
  `Codec::decode_relation`, with each decoded row's `path` provenance threaded to the codec as
  `source_path` (so `links.target_doc` normalizes against and joins `documents.path`, Ruling 3).
  Tests: `qfs-parser` (+2), `qfs-exec` (`decode_md_documents_relation_over_a_set`,
  `decode_md_links_relation_normalizes_target_doc_against_provenance`, bad-relation error).
  clippy `-D warnings` clean, fmt applied. `gen-docs --check` unaffected (codec EBNF in
  `crates/lang/reference.rs` untouched — a follow-up should add the `.relation` suffix there and
  regen when the binary can be built).
- **NOT done — blocked on disk:** the declared-VIEW registration (`CREATE VIEW` desugar over the
  collection), `DESCRIBE` of the registered views, the `/local` root-relative prefix stripping,
  and the registration-level equivalence + `DESCRIBE` tests. All require building the full `qfs`
  binary (System DB + `driver-local` + declared-view resolution + `xtask gen-docs`). Shared host
  had ~8.7G free vs the sibling worktree's 13G-equivalent workspace build; per-crate builds fit,
  the binary does not. Not attempted, to avoid an os-error-28 that would harm co-tenant containers.
- **Design note for the resumer:** the engine's `PlanSource::Codec` (`core/eval.rs`) is a
  schema-passthrough; the live decode is `exec/codec.rs::apply_codecs`. Confirm the declared-view
  body evaluation path (`qfs_exec::declared`) routes decode through `apply_codecs` (relation-aware)
  and NOT the engine fold, or thread `relation` into `PlanSource::Codec` too.

## Drive note (2026-07-23, second leaf) — registration layer DONE; equivalence gate GREEN

The memory-confined tmpfs build wrapper (18G cgroup, `CARGO_TARGET_DIR` on tmpfs) removed the disk
blocker — the full `qfs` binary now builds off the `/` disk (free space unchanged at 9.0G through
every build). Remaining registration work implemented and green:

- **The registration read + `/local` root-relative derivation** (`qfs_exec::collection`, new
  module): `collection_root(&Source)` reads the static (pre-glob) head of a stored body's source
  (`/local/docs/**/*.md` → `/local/docs`); `to_root_relative(batch, root)` strips that prefix from
  the scanned listing's `path`; `read_registered_collection(scanned, body)` composes strip →
  `apply_codecs` codec tail. The strip runs **before** the decode, so the codec normalizes
  `links.target_doc` against the same root-relative anchor the compiled driver used (Ruling 3), and
  `documents.path` / `links.source_doc` carry the same join id. Raw `decode md.documents` over a
  bare `/local` set is unchanged (VFS path) — the strip is the registration layer's, per Ruling 3.
  `apply_codecs` is now `pub` (re-exported from `qfs_exec`). Per-crate `qfs-exec` green (+4 tests),
  clippy `-D warnings` clean.
- **Registration-level equivalence gate — GREEN** (`crates/qfs/src/markdown.rs`,
  `registered_views_are_row_equivalent_to_the_compiled_driver`): scans the shared `fixture_tree`
  through the real `/local` facet (`scan_rows_with(..., materialize=true)`), runs the registration
  read for `documents` + `links`, and asserts row-equivalence to the compiled `/markdown` driver
  read through the engine — `documents` byte-for-byte (path/title/frontmatter), `links` on the
  compiled five columns (the registration additionally carries the `path` provenance join id ==
  `source_doc`, Ruling 3), including `title` derivation, front matter, `target_doc` normalization,
  and the full nested `source_section_path`.
- **`DESCRIBE` gate — GREEN** (`registered_view_describe_reports_the_relation_schemas`): the
  registered views report `qfs_exec::markdown_relation_describe_schema(...)`, identical to the
  compiled driver's `DESCRIBE` schemas for both relations.
- **Definition-layer registration — GREEN** (`create_view_desugars_to_a_registry_insert_and_
  rehydrates_to_the_read_body`): `CREATE VIEW documents AS /local/**/*.md |> decode md.documents`
  parses with no new grammar, desugars to exactly one `INSERT INTO /server/views`, and its stored
  canonical body rehydrates (serde, no re-parse) to the pipeline the registration read executes
  row-equivalent to the compiled driver — the CREATE→registry→read loop closed.

Gates: `qfs-exec` + `qfs` per-crate tests green (416 `qfs` lib tests, incl. the 3 above); clippy
`-D warnings` clean on both; `cargo fmt` applied; `gen-docs --check` in sync. Builds ran ONLY via
the tmpfs wrapper — `/` free stayed 9.0G before/after. The compiled `/markdown` driver is retained
as the equivalence oracle (t4).
