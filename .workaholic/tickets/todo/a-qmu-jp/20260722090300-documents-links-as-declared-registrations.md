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
