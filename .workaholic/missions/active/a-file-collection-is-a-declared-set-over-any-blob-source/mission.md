---
type: Mission
title: A file collection is a declared set over any blob source
slug: a-file-collection-is-a-declared-set-over-any-blob-source
status: active
created_at: 2026-07-20T15:12:30+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: integrations-are-declared-not-compiled
drive_authorized: true
tickets: []
stories: []
concerns: []
gate_type:
gate_target: a WHERE over front-matter columns of a collected file set
gate_assert: North star, not a machine check — pointing qfs at a local directory, collecting its Markdown/JSON/YAML files through the ordinary query pipeline, and filtering with WHERE on columns parsed from a Markdown file's YAML front matter returns the right rows through the engine; and the documents/links surface the viewer needs is a declared view over this same generic path, not a specialized compiled driver. Verified per ticket, not by reading a page.
---

# A file collection is a declared set over any blob source

## Goal

**Collecting files and querying their parsed contents is the generic path, done once — not a
per-file-kind compiled driver each.** The owner's direction (2026-07-20, verbatim requirements):

1. a simple `/local` path pointing at a local directory;
2. under it, collect Markdown, JSON, YAML, or whatever files over the qfs query engine's
   pipelines;
3. for collected articles, apply a `WHERE` clause that filters on fields parsed out of a
   Markdown file's YAML front matter (front matter parsed → its keys are filterable columns).

This must be settled **before** niche drivers multiply, because it decides the shape every
file-kind rides: a photo's EXIF, a CSV, a PDF are the same move as markdown once the collection
model is generic.

**What is true today — verified against the source, not the docs.**

- Requirement 1 is real: `/local` is a sandboxed blob namespace (`crates/driver-local`), and a
  **single-file** read carries a `content` Bytes column that `|> decode md` turns into
  front-matter-keys-as-columns plus a `body` column
  (`crates/codec/src/codecs/markdown.rs`). Cross-source, since codecs are driver-independent.
- Requirements 2 and 3 are **taught but not implemented**. `docs/query-cookbook.md` (the
  Codecs section) teaches `/local/.workaholic/tickets/todo/*.md |> decode md |> where status ==
  'todo'` and a dozen siblings — but a glob/directory listing carries **no `content` column**
  (`driver-local/src/read.rs:46` — "Directory and glob listings fall through … no content
  column") and `DECODE` **rejects any batch that is not exactly one row**
  (`exec/src/codec.rs:111-140`, `decode_needs_single_blob`). The cookbook ratchet is
  parse-only, so this docs-vs-binary divergence shipped silently. The generic collection path
  is a **defect fix as much as a feature**: the recipes the skills already teach agents must
  become executable truth.
- Meanwhile a **specialized** builtin absorbed part of the need: the `/markdown` collection
  driver (`crates/driver-markdown`, blueprint §13b, mission
  `markdown-trees-are-queryable-as-documents-and-links-tables`) resolves a declared root to two
  fixed tables — `documents` (front matter as ONE json column) and `links` (the section-path
  link graph qfs-viewer needs for backlinks). Blueprint §13b records the open question this
  mission exists to answer: *builtin driver, or one alias-registration set?*

**The rulings this mission carries** (design settled here; spelling and mechanics are ticket
work):

1. **§13b is resolved toward the generic path: alias registration wins.** A collection is a
   **declared, named set registered over other paths** (`/local`, `/s3`, `/drive`, `/git`, a
   `union`) — a stored view, created through the existing definition layer with **zero new
   grammar** (§3: `CREATE <noun>` already desugars to an `INSERT` into a registry path; a set
   definition is a pipeline, not a grammar addition). `/markdown` is demoted from builtin to
   one instance of the general shape; other file kinds fall out of the same registration.
2. **`DECODE` over a collected set runs per row — the runtime-semantic hard break.** The
   codec's `bytes↔rows` contract applies to **each row's bytes** of a multi-row
   content-bearing set, and the per-file relations union. The single-file case becomes the
   one-row instance of the same rule; the `decode_needs_single_blob` refusal retires. qfs is
   experimental: this is a redefinition, not a migration.
3. **Provenance is part of the decode-application contract, not each codec's job.** Decoding
   a collected set carries the source address through as a column (the root-relative `path`,
   the canonical join id), so `documents.path`-style joins, backlink derivation, and
   "which file said this" survive the decode. A collected set feeding a decode materializes
   each file's bytes (plan-driven — the engine knows a decode follows the collect).
4. **Interpretation is codec work; the two-relation shape stays.** The markdown
   interpretation — ATX headings, inline links, the **full nested `section_path`** — rehomes
   from the driver crate into the codec layer as a **second declared relation of the same
   format** (the exact surface — a relation-qualified format name, a relation argument to
   `decode`, or codec-declared named outputs — is the first ticket's design brief). `decode
   md` keeps yielding the flat per-document relation; the link relation is reachable
   per-file through the same codec machinery. §13b's crux ("whether `decode markdown` yields
   the two relations or one flat relation") is answered: **both relations exist, each named,
   neither inferred** — a flat single relation would amputate the link graph, and one fused
   relation would conflate a document row with an edge row.
5. **The section-path link graph is a preserved capability, gated by row-equivalence.**
   `documents` and `links` become declared views over the generic path; they must be
   **row-equivalent to the compiled `/markdown` driver on the same fixture trees** (the §13
   twin-and-retire ratchet aimed inward). Only then does the compiled driver retire.
   qfs-viewer's backlinks/relations depend on `links.section_path`; losing it is a regression,
   not a simplification.

## Scope

**Done when** every acceptance item below is ticked: the owner's three requirements hold as
hermetic engine-level tests, the §13b ruling is recorded in the blueprint, and the
`documents`/`links` surface is a declared registration over the generic path proven
row-equivalent to the compiled driver, which then retires.

**Out of scope** (deliberately):

- **The relation vocabulary and link typing** (`parent`/`concerns`/`references` …, declare-and-
  reject). That stays the later mission §13b already names, layered on the preserved
  `section_path`.
- **Write semantics for collections** (front-matter/section UPSERT through a registered set).
  The single-file `decode md |> set … |> encode md |> upsert` round-trip stays as is.
- **Non-markdown interpretation instances** (EXIF, CSV-collection, PDF). The model must make
  them *possible*; shipping them is not this mission.
- **qfs-viewer changes.** The viewer consumes whatever `describe` reports; its repo does its
  own work once the tables move.
- **Declared-driver DSL breadth** — the wire-facing expressiveness work is the sibling mission
  `the-declared-driver-dsl-covers-the-compiled-drivers-concisely`, which consumes this
  mission's codec rulings.

## Experience

- An operator (or agent) points qfs at a directory and queries the files' *contents* as a
  table with no setup beyond the mount: `/local/docs/**/*.md |> decode md |> where owner ==
  'a@qmu.jp' |> select path, title, status` returns one row per matching file, front-matter
  keys as columns, `body` as prose, `path` naming the file — the cookbook recipes as written,
  now executing.
- The same pipeline shape works over any blob source (`/s3`, `/drive`, `/git@ref`) because the
  codec never knew the driver; only the collect segment changes.
- A recurring collection is **registered once as a named set** (a stored view over the
  pipeline) and addressed by its path thereafter; `DESCRIBE` reports its schema, so the viewer
  and agents discover it generically. Registration is an ordinary previewed, audited
  definition-layer write — reviewable, committed, reconciled by `qfs plan`/`qfs apply` (§16)
  like every other definition.
- The markdown knowledge surface is two such registered views — `documents` and `links` — with
  every link row still carrying the full nested heading path; backlinks derive by plain JOIN.
  Nothing the viewer could do before is lost; the specialized driver is gone.

## Acceptance

- [ ] **Requirement 1 (verbatim gate): (#20260722090200-per-row-decode-over-collected-sets.md) "a simple `/local` path pointing at a local
      directory."** A `/local` mount over a local directory is the collection's source; a
      hermetic engine-level test collects a fixture tree through it with no mechanism beyond
      the mount and the pipeline.
- [ ] **Requirement 2 (verbatim gate): (#20260722090200-per-row-decode-over-collected-sets.md) "under it, collect Markdown, JSON, YAML, or whatever
      files over the qfs query engine's pipelines."** `DECODE` over a multi-row collected set
      decodes per row and unions: hermetic tests cover a `*.md` glob (one row per file), a
      `*.json`/`*.yaml` set, and the provenance `path` column on every decoded row;
      `decode_needs_single_blob` is retired with the single-file case passing as the one-row
      instance.
- [ ] **Requirement 3 (verbatim gate): (#20260722090200-per-row-decode-over-collected-sets.md) "for collected articles, apply a `WHERE` clause that
      filters on fields parsed out of a Markdown file's YAML front matter."** A hermetic test
      runs `… *.md |> decode md |> where <front-matter key> == …` over a fixture tree and gets
      exactly the matching files' rows; sparse keys (a file missing the key) read as null, not
      an error.
- [x] **The §13b ruling is recorded.** (#20260722090100-design-brief-codec-relation-surface-and-13b-ruling.md) Blueprint §13b's "builtin driver, or one
      alias-registration set" open question is closed in the blueprint text as ruled above
      (alias registration; `/markdown` demoted to an instance; zero new grammar via §3), and
      the codec-relation surface (how the link relation is named/reached) is ruled in a
      design brief before implementation.
- [ ] **`documents`/`links` are declared registrations (#20260722090300-documents-links-as-declared-registrations.md) over the generic path,
      row-equivalent.** On the same fixture trees, the declared views reproduce the compiled
      `/markdown` driver's `documents` and `links` rows — including `title` derivation,
      front matter, `target_doc` normalization, and the full nested `section_path` (JSON
      array, pre-heading `[]`) — proven by a hermetic equivalence test; `DESCRIBE` reports
      both schemas.
- [ ] **The compiled `/markdown` driver retires on the ratchet.** (#20260722090400-retire-the-compiled-markdown-driver.md) Once the equivalence test is
      green, `crates/driver-markdown`'s driver surface is deleted (its pure parser survives
      wherever the codec layer homes it), `CONNECT … TO markdown` maps onto the registered-set
      shape or is retired with it, and docs/skills regenerate (plugin version bump per
      CLAUDE.md if a taught surface moves).
- [ ] **The cookbook stops teaching what the binary rejects.** (#20260722090500-cookbook-collection-recipes-execution-checked.md) Every multi-file `decode`
      recipe in `docs/query-cookbook.md` / the cookbook articles either executes against a
      hermetic fixture in the test ratchet (execution-checked, not parse-only, for at least
      the collection recipes) or is corrected; docs-true is restored on this surface.

## Changelog

- 2026-07-20 — Mission placed by the design session (owner strategic direction, 2026-07-20:
  settle the generic local file-handling path before niche drivers; this is the
  parallel/preparatory track — qfs-viewer stays priority #1 and is not this mission). Grounded
  in blueprint §3/§4/§5/§6/§13/§13b/§16 and verified against the source: the
  multi-file-decode gap (`exec/src/codec.rs:111-140`, `driver-local/src/read.rs:46`) and the
  cookbook divergence were reproduced by reading, not assumed. Rulings 1-5 in ## Goal are the
  design session's judgment, recorded for the driving session to execute or overturn with
  cause. No tickets cut yet — a claiming session interrogates and cuts its own;
  `drive_authorized` deliberately left empty (no per-ticket interrogation has happened).
- 2026-07-22 — ticket added — 20260722090100-design-brief-codec-relation-surface-and-13b-ruling.md
- 2026-07-22 — ticket added — 20260722090200-per-row-decode-over-collected-sets.md
- 2026-07-22 — ticket added — 20260722090300-documents-links-as-declared-registrations.md
- 2026-07-22 — ticket added — 20260722090400-retire-the-compiled-markdown-driver.md
- 2026-07-22 — ticket added — 20260722090500-cookbook-collection-recipes-execution-checked.md
- 2026-07-22 — mission replanned for the overnight run - five tickets cut per the design rulings, per-ticket judgment pre-answered in interrogation (links surface delegated to the design brief; compiled /markdown deletion plus plugin bump authorized once equivalence is green); drive_authorized stamped — mission.md
- 2026-07-22 — strategy created and linked — integrations-are-declared-not-compiled
- 2026-07-22 — ticket archived — 20260722090100-design-brief-codec-relation-surface-and-13b-ruling.md
