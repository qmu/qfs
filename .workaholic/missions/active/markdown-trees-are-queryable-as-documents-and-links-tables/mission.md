---
type: Mission
title: Markdown trees are queryable as documents and links tables
slug: markdown-trees-are-queryable-as-documents-and-links-tables
status: active
created_at: 2026-07-17T00:05:09+09:00
author: a@qmu.jp
assignee:
drive_authorized:
tickets: []
stories: []
concerns: []
gate_type:
gate_target: documents / links tables of a declared markdown root
gate_assert: North star, not a machine check — pointing qfs at any repository's markdown tree and running DESCRIBE plus a SELECT over its documents and links tables returns real rows through the engine, with every link row carrying the nested heading path of the section it was written under. Verified per ticket, not by reading a page.
---

# Markdown trees are queryable as documents and links tables

## Goal

**A tree of markdown files is an ordinary queryable qfs surface** — an indexer scans a root
directory's `.md` files and resolves them as **two tables**: `documents` (one row per file)
and `links` (one row per markdown link, carrying *where in the document* the link was
written). This is the minimal version of what the strategy plan calls the **markdown
collection path（マークダウン収集パス）**.

The 正本 (canonical) background is the strategy repository, `docs/plan.md`, sections
**「マークダウン収集パス」** and **「トレイル」**. This mission is written to be driven
without reading that document, so the load-bearing background is restated here:

- **qfs-viewer needs this path.** qfs-viewer (a plgg-based viewer product, the successor to
  the old InsightBrowser) renders any qfs path generically from what `describe` returns — a
  `(declaration, rows)` protocol. Its "Insight" preset is markdown browsing. For that to
  exist at all, qfs must resolve a markdown tree as tables whose schema `describe` reports.
  The viewer will point this path at **arbitrary repository roots** — not one blessed repo.
- **Sections are the future relation carrier.** In the strategy model, *a heading is a
  field, and links under it are that field's references*. A closed relation vocabulary
  (`parent` / `concerns` / `references` / …) will later type those links — "declare and
  reject, never guess"（推測するな、宣言して拒否せよ）— and derived reverse edges become
  backlinks and trail segments (`/resolve/docs/plan.md/references/…`). **None of that is
  this mission** — but it only stays possible if the minimal version records, for every
  link, the full nested heading path of the section containing it. Dropping or flattening
  that column now would amputate the later missions.
- **Why two tables, not rendered HTML.** Collecting and interpreting the tree in one qfs
  path means the viewer, AI agents issuing qfs-query directly, qfs automation, and
  cross-service JOINs all reach the *same* facts — no second indexer drifting from the
  first.

## Scope

**Done when** every acceptance item below is ticked: the path's shape and root declaration
are ruled on the declared-drivers convention, a scanned tree resolves as `documents` and
`links` tables through the engine, every link row carries a lossless nested
`source_section_path`, `describe` reports both schemas and an engine-level test proves the
tables are actually reachable, a rescan entry point exists, and hermetic tests pin the
behavior.

**Out of scope** (deliberately — these are later missions or other repos' work, layered on
what this mission must preserve):

- **The relation vocabulary and link typing.** No closed set of declared headings, no
  typed edges, no "reject undeclared" diagnostics, no derived reverse edges. The `links`
  schema carries **no relation-type column** in this version; typing arrives as a separate
  mission over the preserved `source_section_path`.
- **Editing documents through this path.** Read-only in the minimal version; write
  semantics (frontmatter/section UPSERT under describe → preview → commit) come later.
- **Trail / `/resolve` segments and backlink derivation in qfs-viewer.** The viewer's
  lowering and UI are qfs-viewer's repo; this mission only guarantees the tables and
  section paths they will consume.
- **Hot reload / file watching.** Optional; a *manual* rescan entry point is required
  (below), continuous change detection is not.

## Experience

- The operator declares a markdown root (any repository checkout, e.g. a docs tree or a
  `.workaholic/` knowledge bundle) and the tree becomes two queryable tables under one
  path. **How** the root is named — declaration locator, CLI argument, config — is this
  repo's design judgment; the *requirement* is only that qfs-viewer can point it at
  arbitrary repo roots, and that whatever mechanism ships rides the declared-drivers
  convention (a reviewable declaration you commit; no name-shaped `QFS_*` env var as the
  working path; secrets, if any ever apply, referenced not inlined). Path and table naming
  keeps the strategy vocabulary one-to-one: the tables are named `documents` and `links`.
- **`documents`** answers listing and detail: one row per `.md` file with at least the
  root-relative `path` (the canonical id other rows join on), the document `title`
  (frontmatter title or first heading), and frontmatter-derived summary columns sufficient
  for a list view and a detail header. Column set beyond that minimum is implementation
  judgment.
- **`links`** answers "what does this document reference, and from where": one row per
  markdown link with at least
  - `source_doc` — the linking document's `documents.path`,
  - `source_section_path` — the **nested heading path** of the section containing the
    link, from the top-level heading down to the nearest one (e.g. the link under
    「懸念」 inside 「全体の振り返り」 carries both levels, in order). The encoding
    (delimiter, JSON array, …) is implementation judgment but must be lossless (no
    ambiguity with heading text) and documented; a link before any heading carries the
    empty path. **This column is the whole point of the minimal version** — it is the
    context the later vocabulary mission types, so it is never guessed, collapsed to only
    the nearest heading, or dropped.
  - `target` — the link target as written,
  - plus implementation columns as needed (e.g. a normalized in-tree target so that
    relative links joining `links.target` → `documents.path` work, which is how a viewer
    or a query derives outbound links and backlinks today, untyped).
- `DESCRIBE` over the path returns both tables' schemas, so qfs-viewer's **generic
  describe-lowering renders them with no markdown-specific code in the viewer**, and any
  agent can discover the surface before querying it.
- After files change, an explicit rescan entry point re-indexes the root and subsequent
  queries see the new state.

## Acceptance

- [ ] **Path shape and root declaration are ruled and recorded.** A design brief/blueprint
      entry fixes the path name and how a root is declared, riding the declared-drivers
      convention (see mission `declared-drivers-are-the-normal-way-to-add-a-service`: a
      committed, reviewable declaration is the normal way; no `QFS_*` env var as the
      working path; use the current declaration seam or `path_binding` registry as that
      mission has left it by then). The requirement stated, the mechanism qfs's judgment;
      multiple roots on one box must be possible since qfs-viewer targets arbitrary repos.
- [ ] **`documents` resolves through the engine.** Scanning a declared root yields one row
      per `.md` file with root-relative `path`, `title`, and frontmatter-derived summary
      columns sufficient for listing and detail views.
- [ ] **`links` resolves with the section context preserved.** One row per markdown link
      with `source_doc`, `source_section_path` (full nested heading path in order; lossless
      documented encoding; empty for a pre-heading link), and `target` as written; in-tree
      relative targets are joinable against `documents.path` (directly or via a normalized
      implementation column), so outbound links and backlinks are derivable by plain query.
- [ ] **No typing, by construction.** The shipped `links` schema carries no relation-type
      column and the indexer infers no semantics from heading text; the docs for the path
      state that the closed relation vocabulary is a later, separate mission layered on
      `source_section_path`.
- [ ] **`describe` is true AND the tables are reachable.** `DESCRIBE` returns both tables'
      schemas, and an engine-level test SELECTs from both tables through the shell/engine —
      not the indexer struct directly — guarding the describe-registry/mount-registry split
      that once let a driver ship describable but unqueryable (see the `/claude` mission's
      findings).
- [ ] **A rescan entry point exists.** After adding, editing, and removing files under the
      root, invoking the rescan makes subsequent queries reflect the change; hot
      reload/watching stays optional and unshipped without penalty.
- [ ] **Hermetic tests pin the behavior** over a fixture tree: a link under nested headings
      yields the multi-level `source_section_path`; a pre-heading link yields the empty
      path; frontmatter parses into the summary columns; non-`.md` files are ignored;
      rescan reflects a modification; and the workspace gates pass (`cargo test`, clippy,
      fmt, `gen-docs --check`).

## Changelog

- 2026-07-17 — Mission placed by HQ (strategy repo, qfs-viewer MVP headquarters; HQ ticket
  `20260716212003-place-markdown-collection-path-mission-in-qfs.md`). Unclaimed
  (`assignee` empty) — a qfs session claims it and cuts its own tickets. Background 正本:
  strategy `docs/plan.md` sections 「マークダウン収集パス」 and 「トレイル」, restated
  self-containedly above. Scope deliberately excludes the relation vocabulary/typing
  (later mission) while making the nested `source_section_path` column non-negotiable.
  Gate left thin at the start per this repo's convention; `gate_target`/`gate_assert`
  stand as the north star.
