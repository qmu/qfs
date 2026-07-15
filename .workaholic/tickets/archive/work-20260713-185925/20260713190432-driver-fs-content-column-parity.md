---
created_at: 2026-07-13T19:04:32+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 0.5h
commit_hash: aa37d6a
category: Changed
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Give /fs the single-file content read + describe parity /local got in v0.0.60

## Overview

The v0.0.60 `/local` fix (`d3289df`) made the blob node advertise a nullable `content` column at
plan time and materialise the file's bytes on a single-file read, so `/local/<file> |> select
content |> transform` type-checks and runs. `driver-fs` (`QFS_FS_<NAME>` named roots, mounted at
`/fs`) is templated structure-for-structure on `driver-local` but was scoped out of that branch, so
it still has the pre-v0.0.60 behaviour: `describe()` reports the metadata-only `FsRow::schema()`
(no `content`), and `read::scan_rows` never materialises bytes even for a single-file path. The
result is the identical defect: `/fs/<file> |> select content |> transform` fails plan-time
`UnknownColumn`, and even if it type-checked the runtime row would carry no bytes.

This closes concern `37-new-driver-fs-content-omission.md` by mirroring the `/local` fix onto
`driver-fs`.

## Key Files

- `packages/qfs/crates/driver-fs/src/row.rs` — add `FsRow::content_schema()` (listing columns +
  nullable `content` Bytes), mirroring `LocalRow::content_schema()`.
- `packages/qfs/crates/driver-fs/src/read.rs` — add `scan_file_content` (single regular file →
  listing row + `Value::Bytes`), have the listing path append a null `content`, and return
  `content_schema()` everywhere so plan==runtime.
- `packages/qfs/crates/driver-fs/src/lib.rs` — `describe()` returns `FsRow::content_schema()`.
- `packages/qfs/crates/driver-fs/src/tests.rs` — adjust/extend for the widened schema.

## Implementation Steps

1. `FsRow::content_schema()` = `schema()` columns + `Column::new("content", ColumnType::Bytes,
   true)` — a copy of `LocalRow::content_schema()`.
2. In `read.rs`, add `scan_file_content(roots, vfs)`: return `None` for globs, directories, and
   missing paths; for a single regular file, read bytes via `fs_core::read_blob` and augment the
   `resolve_glob` listing row with `Value::Bytes(bytes)`, schema `FsRow::content_schema()`.
   `scan_rows` tries it first, then falls through to the listing path which now appends
   `Value::Null` per row and uses `content_schema()`.
3. `describe()` returns `FsRow::content_schema()`.
4. Update `read.rs`/`tests.rs` assertions from 6 to 7 listing columns; assert a single-file read
   carries `Value::Bytes` and a directory listing carries a null `content`, mirroring the
   `/local` tests (`directory_listing_has_a_null_content_column`, the content-read test).

## Considerations

- **describe() purity / gen-docs**: `describe()` stays pure and path-agnostic (advertises the WIDER
  schema). `/fs` is a BlobNamespace in `docs/drivers.md`, which renders verbs/pushdown but **no
  column list** for blob nodes, so `gen-docs --check` stays clean (confirmed: the `/local` widen
  did not touch drivers.md).
- **Confinement unchanged**: `scan_file_content` resolves through the same `FsRoots::resolve` /
  `read_blob` seam that already validates unknown-root / `..` / symlink escapes; no new path
  reaches the disk without validation.
- No plugin/skill surface change (the taught PDF-extraction recipe is `/local`/`/drive`, not `/fs`),
  so no plugin re-version is owed for this ticket on its own.

## Policies

Derived from `layer: [Domain]` → `workaholic:implementation` (plan/runtime schema agreement, driver
confinement).

## Quality Gate

- **Acceptance**: `/fs/<file> |> select content` type-checks (describe advertises `content`); a
  single-file `/fs` read returns a `content` Bytes value; a directory/glob listing returns a
  present-but-null `content`; confinement errors (unknown_root/outside_root) are unchanged.
- **Verify**: `cargo test -p qfs-driver-fs`, `cargo clippy -p qfs-driver-fs --all-targets -- -D
  warnings`, `cargo fmt -p qfs-driver-fs`, and `cargo run -p xtask -- gen-docs --check`.

## Final Report

Development completed as planned; the fix mirrors the `/local` v0.0.60 widen 1-for-1 onto
`driver-fs`. Verified: `cargo test -p qfs-driver-fs` (33 lib + 3 e2e pass, incl. the two new
content assertions), `cargo clippy -p qfs-driver-fs --all-targets -- -D warnings` clean,
`cargo fmt -p qfs-driver-fs` applied, `cargo run -p xtask -- gen-docs --check` in sync. Closes
concern `37-new-driver-fs-content-omission.md`.

### Discovered Insights

- **Insight**: `driver-fs` had **no single-file content path at all** before this — unlike
  `/local`, whose `read.rs` already carried a `scan_file_content`; the `/fs` scan only ever produced
  metadata `FsRow`s regardless of path shape. The fix adds that path, reusing the existing
  `fs_core::read_blob` primitive (already present for the applier's copy/verify leg).
  **Context**: the two drivers are "templated structure-for-structure" but the templating had
  drifted — `/local` gained content-read in v0.0.60 and `/fs` did not, so a future parity audit
  should diff `driver-local/read.rs` against `driver-fs/read.rs` directly.
- **Insight**: `docs/drivers.md` renders **no column list** for a `BlobNamespace` node (only verbs
  and pushdown), so widening a blob driver's describe schema is `gen-docs --check`-safe — the same
  reason the `/local` v0.0.60 commit left drivers.md untouched.
  **Context**: schema-only changes to `/local`, `/fs`, `/s3`, `/r2`, `/drive` blob nodes never
  require a docs regen.
