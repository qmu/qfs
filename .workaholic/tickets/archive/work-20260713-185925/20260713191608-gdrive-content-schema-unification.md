---
created_at: 2026-07-13T19:16:08+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 1h
commit_hash: 87340d7
category: Changed
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Unify the gdrive file-content and folder-listing schemas so /drive/<file> select content type-checks

## Overview

`driver-gdrive` has TWO divergent read schemas: a folder listing yields the 11-column
`FileMeta::schema()` (`id, name, mime_type, parents, size, modified_time, md5, is_google_doc, rev,
drive_id, trashed`), while a single-file content read (`read::content_batch`) yields a DIFFERENT
5-column schema (`name, mime_type, size, md5, content`). `describe()` reports the folder schema, so
`/drive/<file> |> select content |> transform` fails plan-time `UnknownColumn` (`content` is not in
the described columns) — round 5 needed a struct bypass. This mirrors the pre-v0.0.60 `/local`
defect, but gdrive's fix needs the file-vs-folder schemas *unified* first (they genuinely differ),
not just a `content` column appended.

Closes concern `37-new-drive-select-content-schema-divergence.md`.

## Key Files

- `packages/qfs/crates/driver-gdrive/src/schema.rs` — add `FileMeta::content_schema()` (the 11
  listing columns + a nullable `content` Bytes column).
- `packages/qfs/crates/driver-gdrive/src/read.rs` — converge both read paths on `content_schema()`:
  `folder_batch` appends a null `content` per row; `content_batch` returns the file's full listing
  row (`meta.to_row()`) plus the downloaded/exported bytes under `content`.
- `packages/qfs/crates/driver-gdrive/src/lib.rs` — `describe()` returns `FileMeta::content_schema()`.
- `packages/qfs/crates/driver-gdrive/src/tests.rs` — assert `content` is in the described schema.

## Implementation Steps

1. `FileMeta::content_schema()` = `schema()` columns + `Column::new("content", ColumnType::Bytes,
   true)`.
2. `describe()` returns `FileMeta::content_schema()`.
3. `folder_batch(rows)` widens each listing row with a trailing `Value::Null` and uses
   `content_schema()` — plan==runtime for every listing.
4. `content_batch` returns `meta.to_row()` (all 11 canonical columns from the resolved `FileMeta`)
   with `Value::Bytes(bytes)` appended, schema `content_schema()`. A file addressed directly now
   reports the SAME metadata columns it shows in its parent's listing, plus `content` — true
   unification. Drop the ad-hoc 5-column schema and the now-unused `mime` parameter.
5. Update `read_rows` file-read callsites to the new `content_batch(&meta, bytes)` signature.
6. Extend the describe test to assert `content` is present.

## Considerations

- **Metadata semantics on the content row**: the unified content row reports the file's *canonical*
  Drive metadata (source `mime_type`, stored `size` — 0 for a Google-native doc), IDENTICAL to what
  the folder listing shows for the same file; the exported/downloaded bytes live in `content`. This
  is the deliberate consistency of a unified schema (a file's row is the same whether reached by
  listing or by direct address). It changes the previous content-row behaviour, which reported the
  *export-target* mime and the *received* byte count — no test asserted those values, and the
  listing-consistent metadata is the more coherent contract.
- **gen-docs**: `/drive` is a `BlobNamespace` in `docs/drivers.md`, which renders no column list for
  blob nodes, so `gen-docs --check` stays clean.
- **No new network / auth surface**; the read/apply seams are untouched.
- The exec-layer oneshot tests use an independent fake Drive schema, not `driver-gdrive`, so they
  are unaffected.

## Policies

Derived from `layer: [Domain]` → `workaholic:implementation` (plan/runtime schema agreement).

## Quality Gate

- **Acceptance**: `describe()` advertises a nullable `content` column; a `/drive/<file>` content read
  returns rows in the unified 12-column schema with `content` populated; a folder listing returns
  the same 12-column schema with a null `content`; `select content` type-checks at plan time.
- **Verify**: `cargo test -p qfs-driver-gdrive`, `cargo clippy -p qfs-driver-gdrive --all-targets --
  -D warnings`, `cargo fmt -p qfs-driver-gdrive`, and `cargo run -p xtask -- gen-docs --check`.

## Final Report

Development completed as planned; both gdrive read paths (file-content and folder-listing) now
converge on `FileMeta::content_schema()` (12 columns), so `describe` == listing == single-file read.
Verified: `cargo test -p qfs-driver-gdrive` (51 pass, incl. the extended describe test asserting a
nullable `content`), `cargo clippy -p qfs-driver-gdrive --all-targets -- -D warnings` clean,
`cargo fmt -p qfs-driver-gdrive` applied, `cargo run -p xtask -- gen-docs --check` and
`gen-skills --check` both in sync. Closes concern `37-new-drive-select-content-schema-divergence.md`.

### Discovered Insights

- **Insight**: the gdrive divergence was deeper than `/local`'s — the file-content read used a
  wholly separate 5-column schema (`name/mime_type/size/md5/content`), not merely a listing missing
  `content`. Unifying meant making the content row report the file's OWN listing metadata
  (`meta.to_row()`), which incidentally changed two previously-special values: the content row now
  reports the Drive source `mime_type` (not the export-target mime) and the stored `size` (not the
  received byte count). No test asserted those, and listing-consistency is the more coherent
  contract, but a caller who relied on the direct read reporting the exported bytes' size/mime would
  see the change.
- **Insight**: the gdrive cookbook prose already claimed `content` on a single-file read and told
  readers to `qfs describe /drive/my` for the schema — which, pre-fix, would NOT have shown it. The
  doc was aspirational; this fix makes it true, so no cookbook edit (and no plugin re-version) is
  owed by this ticket. Contrast the sibling `/local` fix, which DID edit its cookbook recipe and
  therefore owed a plugin bump.
