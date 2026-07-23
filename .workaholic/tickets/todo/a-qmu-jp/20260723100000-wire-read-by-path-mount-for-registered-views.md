---
created_at: 2026-07-23T10:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260722090300-documents-links-as-declared-registrations.md]
mission: a-file-collection-is-a-declared-set-over-any-blob-source
---

# wire a read-by-path mount for registered collection views

## Overview

t3 (20260722090300, commit c6d834d) shipped `qfs_exec::collection::read_registered_collection`
and proved `documents`/`links` row-equivalent to the compiled `/markdown` driver — but ONLY at
the **registration-read-helper** level. Nothing in the binary resolves a registered collection
view **by path** yet, and the `/local` root-relative strip lives only in the helper, not in the
generic `decode md.<relation>` query path (which Ruling 3 keeps VFS-anchored). So the raw pipeline
`/local/docs/**/*.md |> decode md.links` yields VFS-anchored `source_doc`/`target_doc` that do not
self-join — it is NOT a drop-in replacement for the compiled driver's by-path surface the viewer
depends on. This ticket wires the missing production surface; it is the prerequisite for retiring
the compiled driver (20260722090400).

## Scope

Wire a **read-by-path mount** whose read facet runs `read_registered_collection` over a registered
view's declared body (its `/local` scan), applying the root-relative strip, so a **live query** and
`DESCRIBE` reach the declared `documents`/`links` views the way the compiled `/markdown/<name>`
mount does today — resolving the registered view by path, registered through the existing
definition layer (no new grammar). Prove the LIVE surface (not just the helper) row-equivalent.

## Policies

- No viewer regression: the by-path read surface must be row-equivalent to the compiled
  `/markdown` driver on the same fixtures (title, front matter, `target_doc` normalization, nested
  `source_section_path`), and `links.source_doc`/`target_doc` must self-join against `documents.path`.
- The root-relative strip is applied on this read-by-path facet (as in the helper), NOT bolted onto
  the generic `decode` path (which stays VFS-anchored per Ruling 3).
- No new grammar; registration remains an ordinary definition-layer write.

## Quality Gate

- A hermetic test: a **live query by path** over a registered collection view returns rows
  row-equivalent to the compiled driver on all four dimensions, and `links` self-joins to
  `documents.path`.
- A `DESCRIBE` by path over the registered view reports the same schemas as the compiled driver.
- Builds run ONLY via the memory-capped tmpfs wrapper (below); `cargo test`/`clippy -D warnings`/
  `fmt --check` on the touched crates + `gen-docs --check` all green.
