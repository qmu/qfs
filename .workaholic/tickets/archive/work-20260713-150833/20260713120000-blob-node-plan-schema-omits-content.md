---
created_at: 2026-07-13T12:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Single-file blob nodes omit `content` from the plan-time schema (taught extraction recipe cannot run)

## Problem (found live, round 5, v0.0.59)

The runtime row of a single-file read carries `content` (cookbook-documented, live-confirmed for
/local and — via the struct constructor — /drive), but the plan-time schema lists metadata only:

- `/drive/my/<file> |> select content as blob` → `UnknownColumn { name: "content", available:
  [id, name, mime_type, parents, size, modified_time, md5, is_google_doc, rev, drive_id, trashed] }`
- `/local/<file> |> select content |> transform x |> insert …` → same UnknownColumn (local
  available: name, path, size, modified, is_dir, mode); yet a STANDALONE
  `/local/<file> |> select content` executes and returns the bytes
- `… |> transform <extraction>` → `TransformInputMissing` for both drivers, so **the cookbook's
  PDF-extraction recipe (`/local/report.pdf |> transform extract |> upsert into /drive/...`) is
  unrunnable against the real drivers**
- The struct constructor bypasses the check: `|> select {c: content} as s |> extend content = s.c
  |> select content |> transform …` **plans and commits fine** (round 5 ran the whole extraction
  through this bypass) — so the plan/runtime schema divergence is enforced in some expression
  positions and not others

The hermetic extraction test (`crates/exec/tests/oneshot.rs::pdf_extraction_to_drive`) fakes the
/local driver with a `blob`-column schema, which is why the divergence never surfaced; the
cookbook ratchet is parse-only and cannot catch it either.

## Fix

Make single-file blob node describe() schemas include the `content` bytes column (matching the
documented runtime row), or make the planner treat blob nodes' content as a projectable column.
Then remove the need for the struct bypass, and re-point the hermetic extraction test at the real
/local driver schema so the seam is honest. Consider an execute-one-recipe smoke ratchet for
cookbook recipes whose truth is plan-time (parse-only cannot see type-check refusals).

## Key files

- `packages/qfs/crates/driver-local/` and `driver-gdrive/` — describe() schemas for file nodes
- the planner's projection/transform-input type check (crates/pushdown/src/lower.rs names
  TransformInputMissing)
- `crates/exec/tests/oneshot.rs::pdf_extraction_to_drive` — the faked source schema
- `docs/cookbook/cross-service.md` — the taught extraction recipe

## Acceptance

- The cookbook extraction statement runs verbatim against real /local (hermetic equivalent with
  the true schema) and previews model-free.
- `select content` on a single-file node plans identically standalone and mid-pipeline.

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause: `LocalFsDriver::describe()` is path-agnostic and pure (no I/O — it is the gen-docs
source), and returned the narrow `LocalRow::schema()` (no `content`). The plan-time type-check reads
describe, so `|> select content |> transform` failed `UnknownColumn`/`TransformInputMissing` even
though a single-file runtime read populates `content`. A standalone `select content` slipped through
because it projects over the runtime batch (which HAS content); a transform pipeline type-checks
against describe (which did not), hence the position-dependent divergence.

Fix (`/local`, chosen: describe advertises the wider schema, runtime made to match):
- `describe()` now returns `LocalRow::content_schema()` (listing + nullable `content`), so
  `select content` and the extraction transform type-check at plan time.
- `scan_rows` gives directory/glob listings the SAME schema with `content = null` (a listing does
  not materialise each entry's bytes), so plan-time and runtime schemas agree in every case (no new
  divergence). A single-file read still populates `content` with the bytes.
- Re-pointed the hermetic extraction test (`exec/tests/oneshot.rs::pdf_extraction_to_drive`) off the
  single-`blob` fake onto the REAL `LocalRow::content_schema` shape, and the statement now reads
  `/local/report.pdf |> select content |> transform extract |> upsert into /drive/...` — proving the
  recipe plans + commits against the honest schema (acceptance 1). This is the execute-one-recipe
  smoke the ticket asked to consider.
- Cookbook `cross-service.md` extraction recipe corrected to `|> select content as blob |> transform
  extract` (narrow the multi-column single-file read to the single-`bytes` Extraction input matching
  `input (blob bytes)`); regenerated `qfs-cross-service` SKILL.md; the cookbook parse ratchet stays
  green. **Plugin version bump owed at report time** (taught-surface change).

**Scoped follow-ups (noted, not done here):**
- `/drive` single-file `select content`: gdrive's file-content read schema (`name/mime_type/size/
  md5/content`) is a DIFFERENT column set from the folder listing (`id/name/mime_type/parents/…`), so
  advertising `content` in gdrive describe needs the file-vs-folder schema unified first — a deeper
  change than the /local schema-widen. The round-5 struct bypass remains available for /drive until
  then.
- `driver-fs` (the `QFS_FS_<NAME>` named-roots driver) shares the identical `FsRow` content-omission
  and should get the same widen when prioritized.
