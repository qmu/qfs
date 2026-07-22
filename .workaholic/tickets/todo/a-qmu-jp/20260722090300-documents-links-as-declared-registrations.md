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
