---
type: Mission
title: qfs-viewer MVP
slug: qfs-viewer-mvp
status: active
created_at: 2026-07-17T00:29:05+09:00
author: a@qmu.jp
assignee: a@qmu.jp
drive_authorized:
tickets: [20260717020101-qfs-connection-seam-with-swappable-issuance-forms.md, 20260717020102-describe-generic-browsing-as-default-columns.md, 20260717020103-resolve-addresses-with-prefix-closure.md, 20260717020104-strip-ui-on-the-plggmatic-engine.md, 20260717020105-markdown-browsing-over-the-qfs-collection-path.md, 20260717020106-keep-npx-distribution-true-through-the-ui-replacement.md, 20260717020107-run-the-final-demo-end-to-end.md, 20260716092321-prove-a-plggmatic-ui-can-be-an-mcp-app.md]
stories: []
concerns: []
gate_type:
gate_target:
gate_assert:
---

> Placed by HQ on 2026-07-17 (strategy mission `qfs-viewer-mvp-headquarters`,
> ticket `20260716212004-place-mvp-mission-in-qfs-viewer.md`). This document
> defines the mission only; implementation is this repository's own sessions'
> work. The mission is **unclaimed** — `assignee` is deliberately empty.

# qfs-viewer MVP

## Goal

The **正本 (canonical source) for every concept this mission uses is the
strategy plan: `qmu/strategy` `docs/plan.md`** (published as the qmu.app 計画書
at strategy.qmu.dev). This mission does not re-define パス, トレイル,
接頭辞閉包, マークダウン収集パス, qfs-query, or the three issuance forms —
where a term appears below, the plan's definition governs. If this document
and the plan ever disagree, the plan wins.

Per the plan, **qfs-viewer is the product**: a plgg-based JS server
application that takes qfs as its mandatory substrate and renders/operates
every resource qfs resolves, with plggmatic (the column-strip engine, now
living in this repo at `packages/plggmatic`) as its UI engine. qfs is
mandatory so that collection and interpretation live in one place — a qfs
path — and no second implementation can drift from the first. The dependency
on qfs is a data-plane dependency across a process boundary, not a library
coupling, so it does not violate the plgg family's dependency-minimal rule.

The MVP is the plan's first deterministic rung ("決定的なマニフェスト生成者が
先、LLM は後"): a viewer where **the trail is the address** — the i-th column
of the strip is the resolution of path prefix i, a click appends a segment,
and revisiting the address reproduces the columns. Achieving it proves the
plan's step 1 (汎用ロワリング — generic lowering over describe) end to end
and consumes step 2 (マークダウン収集パス) as it lands on the qfs side.

**The final demo this mission exists to make real** (each leg maps 1:1 onto
an Acceptance item below):

1. At the root of the `qmu/strategy` repository, run `npx qfs-viewer` — the
   viewer starts against that working tree.
2. Browse the markdown under `docs/` as horizontal column strips — documents
   and their links traversed sideways, column by column.
3. Browse a connected qfs resource generically — any path qfs can describe
   renders as a default column view, no per-resource code.
4. Copy the `/resolve` address, revisit it, and see the same columns
   reproduced.

### Premises — what this repository already is

- **Renamed and ported (2026-07-16, HQ ticket 212002 → merge `071814d`).**
  This repo was InsightBrowser; it is now the standalone home of qfs-viewer,
  and the plggmatic engine (Declaration → Scene → renderers, the Flow tier,
  the Tool catalog) was ported into `packages/plggmatic`. The design record
  travels with it: `docs/plggmatic-semantics/screen-structure-mission.md`
  (all 12 semantic decisions), `dsl-v1-core.md` (the frozen Flow DSL), and
  `poc-findings.md` (the two live PoC results).
- **The InsightBrowser-derived asset.** `packages/qfs-viewer` is the working
  markdown browser inherited from InsightBrowser: scanner + front-matter
  index, SSR HTML / REST / MCP over one model, a smoked `npx qfs-viewer` bin
  — built directly on `plgg-view`, with its own column-accretion UI. Under
  this mission its parts have three fates, per the plan: the **bin, server
  shell, and distribution path are kept** (they are the MVP's chassis); the
  **in-process markdown indexer retires into qfs** as the マークダウン収集パス
  (the plan names this explicitly — the old indexer "住み替え", and the name
  InsightBrowser retires); the **hand-built UI is superseded** by the
  plggmatic strip.
- **ADR 0002 must be amended before the strip work lands.**
  `docs/adr/0002-plggmatic-is-a-reference-not-a-dependency.md` and the
  dependency-contract gate currently forbid `packages/qfs-viewer` from
  depending on plggmatic. That stance was InsightBrowser's; the plan makes
  plggmatic the UI engine. Amending the ADR (and the gate) is part of the
  strip-UI acceptance item, not a silent side effect.
- **Two facts are already measured, not hoped**
  (`docs/plggmatic-semantics/poc-findings.md`): the horizontal strip fits a
  bounded 420×640 host frame structurally (8 columns deep, body width
  constant — depth does not consume the viewport), and URL-as-truth fails
  inside host frames (`pushState` throws), so plgg-view needs a virtual-URL
  mode — a separate upstream `/request`, **not** this mission's work; the
  MVP runs standalone with a real address bar, where real URLs are fine.

## Scope

**Definition of done:** the four-leg demo above runs end to end, each
Acceptance item below is checked with tests, and `scripts/check-all.sh`
stays green throughout.

**Out of scope** (deliberately, per the plan and the HQ ticket):

- **Manifest lowering** (plgg-ir `Declaration` → Scene from authored or
  LLM-generated manifests). The MVP ships only the thin generic lowering
  from describe schemas; the plan's step 3 (LLM 生成マニフェスト) and rich
  manifests come later. The implementation must not preclude them — see
  Considerations.
- **Relation-vocabulary declaration and typing.** The closed relation set
  ("推測するな、宣言して拒否せよ") is a strategy open question headed to the
  qfs repo as a path spec; the viewer consumes whatever describe returns.
- **Document editing.** The old InsightBrowser in-browser editing does not
  gate the MVP; editing returns later riding qfs's describe → preview →
  commit safety model, as the plan prescribes.
- **Access control** (RBAC/PBAC over paths, principals, API keys).
- **MCP App embedding.** The imported PoC ticket
  `20260716092321-prove-a-plggmatic-ui-can-be-an-mcp-app.md` (in this repo's
  todo, `mission: qfs-viewer-mvp`) is positioned as this mission's
  **successor**: it proves the same Scene can be served as an MCP App inside
  a first-party vendor's chat, and it starts from this MVP's outcome plus
  the virtual-URL `/request`. It is tracked here so it is not orphaned, but
  it is not an MVP acceptance item.

## Experience

What a developer observes when the mission is done — each numbered behavior
is checkable, and together they are exactly the demo:

1. **Start.** In any repository root — the demo uses `qmu/strategy` —
   `npx qfs-viewer` starts the viewer. By default each query is served by an
   on-demand invocation of the `qfs` binary (issuance form ② オンデマンド起動
   of the plan's three); no daemon needs to be running first.
2. **The strip.** The screen is a horizontal strip of columns with **static
   column headers**; navigation accretes columns to the right instead of
   replacing the screen, so how you arrived stays readable. The strip owns
   its own horizontal scrolling — **depth never consumes the viewport**
   (recursion trails 8+ columns deep leave the page body's width constant,
   as measured in the PoC). This is the shape the plggmatic reference app
   discovered empirically; `docs/plggmatic-semantics/` is the design record.
3. **The address.** The view's truth is a path under `/resolve`:
   **column i is the resolution of the path's prefix i** (接頭辞閉包 — every
   prefix of a valid trail is a valid trail), and **a click is a segment
   appended** to the address. Pasting a `/resolve` address into a fresh
   session reproduces the same columns. Display state (column folding, sort
   order, highlights) is **never** encoded in the address — the address
   determines data, not presentation.
4. **Markdown browsing.** Pointed at a repo, the viewer consumes qfs's
   マークダウン収集パス — the two relational tables (`documents` and
   `links`, the latter carrying `source_section_path`) that the qfs-side
   mission `markdown-trees-are-queryable-as-documents-and-links-tables`
   (already placed in the qfs repo) provides. Following a document's links
   walks the strip sideways: document → links of this document → target
   document …
5. **Generic browsing.** Any other path qfs resolves — a database table, a
   mail folder, whatever is connected — gets a **default column view lowered
   thinly from its describe schema**: rows as a list column, a selected row
   as a detail column. No per-service code; every connected resource is at
   least browsable.
6. **Connection plumbing.** The qfs connection sits behind a small seam:
   on-demand command invocation is the default, and the connection form is
   swappable by configuration (the plan's three issuance forms — local
   server ①, on-demand spawn ②, remote ③ — are the design target; the MVP
   implements ② and only *shapes* the seam for ① and ③).

## Acceptance

Six items plus the end-to-end demo; each names its demo leg. Every item is
sized to be independently `/drive`-able and lands with tests;
`scripts/check-all.sh` must stay green after each. Ticket filenames are
appended as (#…) markers when each ticket is filed.

- [x] **Horizontal strip UI** on the ported engine (`packages/plggmatic`):
      column strip, recursive trail, static column headers, depth not
      consuming the viewport — the measured reference shape per
      `docs/plggmatic-semantics/`; includes the ADR 0002 amendment that
      makes plggmatic this package's UI engine. (demo legs 2–3)
      (#20260717020104-strip-ui-on-the-plggmatic-engine.md)
      — landed 2026-07-17 on the published engine (`plggmatic@0.2.0` from
      the registry); the depth measurement re-run and reproduced.
- [x] **describe generic browsing**: any qfs path's describe schema lowers
      to a default column view (thin, deterministic, no per-resource code).
      (demo leg 3)
      (#20260717020102-describe-generic-browsing-as-default-columns.md)
      — landed 2026-07-17 on the existing plgg-view column UI; the strip
      re-rendering rides item 1's ticket, which the lowering already feeds.
- [x] **Markdown browsing** over qfs's マークダウン収集パス: `documents` /
      `links` (with `source_section_path`) traversed as columns — document →
      its links → target document. Depends on the qfs-side mission
      `markdown-trees-are-queryable-as-documents-and-links-tables`. (demo
      leg 2)
      (#20260717020105-markdown-browsing-over-the-qfs-collection-path.md)
      — landed 2026-07-17 behind the config's `collection` switch
      (docs/adr/0008): qfs's `/markdown/<name>/documents|links` enumerate and
      interpret the corpus, read per request through the PR #5 runner seam,
      and each document column walks its links table sideways with
      `source_section_path` as the link's section context. The in-process
      scanner is inert by construction on that arm (specs run it against a
      throwing filesystem) and dated for deletion 2026-07-31 rather than kept
      as a parallel truth. Live at the qmu/strategy root on qfs 0.0.75: 12
      documents / 72 links, `/resolve/CLAUDE.md,.workaholic/hq-desk-rules.md`
      renders 3 columns, zero `scan.*` events.
- [x] **/resolve addresses**: prefix closure (column i = resolution of
      prefix i), click = segment append, revisiting an address reproduces
      the same columns; display state provably absent from the address.
      (demo leg 4)
      (#20260717020103-resolve-addresses-with-prefix-closure.md)
      — landed 2026-07-17: `/resolve/<trail>` subsumes `?cols=`
      (docs/adr/0007), rendered on the existing plgg-view column UI, which
      the strip ticket re-skins.
- [x] **qfs connection seam**: on-demand command invocation as the default,
      connection form swappable by configuration (skeleton only for the
      other two issuance forms). (demo leg 1)
      (#20260717020101-qfs-connection-seam-with-swappable-issuance-forms.md)
- [x] **Distribution**: `npx qfs-viewer` starts from any repository root
      (the bin exists and is smoke-tested today — keep it true through the
      UI replacement). (demo leg 1)
      (#20260717020106-keep-npx-distribution-true-through-the-ui-replacement.md)
      — landed 2026-07-17: the smoke now starts the PACKED, installed bin
      under node, bun and deno and asserts it serves the engine strip and
      an addressed `/resolve` column. qfs is FOUND, never bundled or
      fetched (docs/adr/0009); with none reachable the viewer still starts
      and says what is missing and how to get it.
- [ ] **The final demo end to end**: at the `qmu/strategy` root, npx start →
      browse `docs/` markdown in columns → browse a qfs resource via the
      generic describe view → revisit a `/resolve` address and see the same
      columns. (all legs)
      (#20260717020107-run-the-final-demo-end-to-end.md)

## Considerations

- **Dependency order — minimize blocking.** The qfs-side markdown-collection
  mission may still be incomplete when work starts here. That blocks only
  the markdown-browsing item: **describe generic browsing and /resolve can
  be implemented first** (any table-shaped qfs path exercises both), and md
  browsing then arrives as "one more describable path" plus its
  link-traversal affordance. Sequence tickets accordingly.
- **/resolve syntax is deliberately not frozen.** The exact grammar of
  `@selection` (composite keys) and the naming of derived reverse edges
  (`~projects`-style) are open questions **owned by strategy** (plan.md
  開いた問い). The MVP stands on **containment segments and simple row
  selection only**, which no answer to those questions will invalidate — do
  not wait for the freeze, and do not invent local answers that would
  compete with it.
- **Sacrificial architecture — the UI is one skin over one Scene**
  (workaholic:design / sacrificial-architecture). The MVP must not preclude
  the lowerings that come after it: the generic describe lowering is *one
  deterministic manifest generator* feeding the same
  Declaration → Scene → renderers pipeline that richer manifests (the
  markdown path's, later an LLM's) will feed. Keep the generator seam
  explicit — describe-to-default-view is a function that later generators
  sit beside, not a hard-wired special case inside the engine. The plan's
  配管は一本 (one pipeline) is the invariant to protect.
- **Do not re-implement collection.** The old in-process markdown indexer
  must not survive as a parallel source of truth next to qfs's collection
  path — that is exactly the drift qfs-mandatory exists to kill. Its
  retirement rides the markdown-browsing item.
- **Virtual-URL mode is upstream work.** The PoC finding that `pushState`
  throws in sandboxed host frames concerns plgg-view and is a separate
  `/request`; the MVP is standalone-browser only and uses real URLs. The
  `/resolve` path is what gives that future serialization its principle.
- **npm install timing.** A fresh clone's `npm install` is blocked by the
  registry's min-release-age until **2026-07-22** for the newest plgg-family
  releases. The developer can lift the hold — if a ticket needs it earlier,
  route the request through HQ rather than pinning stale versions.
- **HQ boundary.** This mission was placed by strategy HQ; HQ does not
  direct the implementation. Claiming the mission (filling `assignee`) and
  filing its tickets is this repository's prerogative.

## Changelog

- 2026-07-17 — Mission placed by HQ (strategy mission
  `qfs-viewer-mvp-headquarters`, ticket
  `20260716212004-place-mvp-mission-in-qfs-viewer.md`); assignee left empty
  (unclaimed). Imported MCP Apps PoC ticket re-pointed to this mission as
  its successor — 20260716092321-prove-a-plggmatic-ui-can-be-an-mcp-app.md
- 2026-07-17 — Mission claimed (assignee a@qmu.jp; the developer approved
  HQ-assigned driving 2026-07-17) and decomposed into six ordered tickets
  mirroring the acceptance items, sequenced per the dependency-order
  consideration (seam → generic browsing → /resolve → strip UI → markdown
  path → distribution → demo); the imported MCP-App PoC ticket stays the
  successor, not duplicated — 20260717020101…020107 (todo/a-qmu-jp)
- 2026-07-17 — qfs connection seam LANDED (acceptance item 5 ticked): the
  connection is a closed `QfsConnection` union parsed from the config's
  `qfs` key (`domain/model/Connection.ts`), on-demand spawn is the
  zero-config default, `local-server`/`remote` are selectable skeletons
  answering with a typed error, the `ResourceRunner` seam gained
  `describe`, and the vendor adapter (`vendors/qfsRunner.ts`) folds the
  union — spec'd against a fixture binary, no real qfs needed. Measured
  note: qfs 0.0.71 answers a local query in ~50ms; the 7.2s startup the
  old timeout comment recorded is gone upstream —
  20260717020101-qfs-connection-seam-with-swappable-issuance-forms.md
- 2026-07-17 — describe generic browsing LANDED (acceptance item 2 ticked,
  honest scope: rendered on the existing plgg-view column UI, not yet the
  plggmatic strip): `qfs:<path>` is a third trail stop, `GET /qfs` is the
  form's translation into a trail segment, and any describable path renders
  as the default column view — describe header (path, archetype), typed
  columns, rows via `<path> |> limit 200`, containment links appending
  segments (`lowerToDefaultView`, the ONE deterministic generator in
  `domain/model/Describe.ts`, kept as the explicit seam later generators
  sit beside). Verified live: /local paths of this repo and qmu/strategy
  browse and deepen in columns, addresses reproduce on revisit —
  20260717020102-describe-generic-browsing-as-default-columns.md
- 2026-07-17 — FOUND during the slice, recorded on the strip-UI ticket: the
  registry's `plggmatic` (0.1.0, 2026-07-04) predates the engine port and
  the ported engine is unpublished, so qfs-viewer cannot honestly consume
  `packages/plggmatic` yet — a `file:` dep breaks the npx smoke, and the
  bin executes `src`, so consumers resolve deps from the registry. The
  strip ticket owns the publish + ADR 0002 amendment + gate change
  sequence — 20260717020104-strip-ui-on-the-plggmatic-engine.md
- 2026-07-17 — /resolve addresses LANDED (acceptance item 4 ticked): the
  view's truth is now a path — `/resolve/<trail>` is the canonical,
  prefix-closed address (column i = resolution of the address's prefix i at
  the comma; a click appends one segment), the trail's ONE serialization
  moved from the `?cols=` query into the path, and a legacy `?cols=`
  request answers 308 to the canonical address so exactly one spelling
  stays in circulation. docs/adr/0007 records the subsume-vs-redirect
  decision, why the comma stays the separator while strategy owns the trail
  grammar (`@selection` etc. deliberately not pre-empted), and why the
  trail parses from the raw pathname (the router's percent-decode would let
  `%2C` forge the separator). Display state is provably absent: the address
  grammar has no parameter slot (codec spec) and no query parameter changes
  which columns an address names (api spec). Verified live: pasting an
  address reproduces the columns byte-for-byte in a fresh session, clicks
  append, legacy bookmarks walk forward with filters kept —
  20260717020103-resolve-addresses-with-prefix-closure.md
- 2026-07-17 — Strip-UI ticket, publish-independent half landed: ADR 0002
  carries the second amendment (plggmatic becomes the UI engine, the full
  sequence recorded) and the dependency-contract gate accepts plggmatic
  while still rejecting every other non-plgg dep. The registry wall
  re-verified: the `plggmatic@0.1.0` tarball contains no code at all
  (package.json + README only). Step 1 — publishing the ported engine
  above 0.1.0 — needs the developer's npm credentials (`npm whoami` is 401
  in the work environment) and is routed via HQ; the dependency flip and
  the strip re-render stay on the ticket, which remains in todo —
  20260717020104-strip-ui-on-the-plggmatic-engine.md
- 2026-07-17 — Horizontal strip UI LANDED (acceptance item 1 ticked): the
  developer published `plggmatic@0.2.0` (real tarball, full dist), the
  publish wall fell, and `packages/qfs-viewer` now consumes the engine
  from the registry (`^0.2.0`). `/` and `/resolve` render the ENGINE
  strip — engine columns in one engine row, sticky `colHead` static
  headers whose title is the collapse link, engine scheme/metric/chrome
  CSS with the html.dark bootstrap — and the trail lowers into the
  engine's `Scene` (`domain/usecase/scene.ts`: corpus → MenuLevel,
  document → DetailLevel, describe default view → ListLevel of exactly
  the containment links) which the engine's own `crumbsOf` folds into
  the breadcrumb rail. The hand-built renderer and its palette retired
  (sacrificial architecture, on schedule). PoC depth measurement re-run
  live: 9 columns deep, body scrollWidth constant (1280 and 420) while
  the strip scrolls 4457px / 4877px internally — depth never consumes
  the viewport. check-all green; the npx smoke resolves the published
  engine from the registry under node, bun, and deno —
  20260717020104-strip-ui-on-the-plggmatic-engine.md
- 2026-07-17 — **Acceptance item 3 (markdown browsing, demo leg 2) landed.**
  The qfs-side dependency cleared (qfs PR #6 merged the collection path), so
  the ticket's "Blocked on" opened. The path contract was verified against
  qfs main rather than trusted: the same day's qfs PR #7 canonicalized the
  **hosts** realm only (`/hosts/<host>/claude`, bare `/claude` retired), and
  `/markdown` is NOT host-realm-only — qfs's generated `docs/drivers.md`
  teaches the bare `/markdown/<name>/…`, so that is the spelling this viewer
  speaks (docs/adr/0008 records the check). The corpus now comes from
  `/markdown/<name>/documents` (enumeration AND front-matter interpretation)
  and each document column walks `/markdown/<name>/links` sideways, with
  `source_section_path` rendered as each link's section context; both arrive
  through item 6's `ResourceRunner` seam, so no markdown-specific transport
  exists. The in-process indexer retires INTO that path behind the config's
  one `collection` switch (both composition roots), with a recorded
  retirement date of **2026-07-31** — the operation policy's coexistence
  allowance spent with the written end it demands. Its inertness is
  structural rather than asserted: the collection specs run against a
  filesystem whose walk surface throws, and the parallel-truth spec pins that
  a file whose fence disagrees with the table facets on the TABLE. Front
  matter became folded plain data (`Document.FrontMatter`) — the shape both
  producers share, so `YamlMap` no longer leaks into facets, filters, REST or
  MCP. Live at the qmu/strategy root (qfs 0.0.75, isolated store, read-only,
  port 4137 — the developer's 4100 untouched): 12 documents / 72 links,
  `/api/health` `{"documentCount":12,"errorCount":0}`,
  `/resolve/CLAUDE.md,.workaholic/hq-desk-rules.md` renders 3 columns, and
  zero `scan.*` events — the scanner never walked. check-all exits 0 —
  20260717020105-markdown-browsing-over-the-qfs-collection-path.md
- 2026-07-17 — story reported — work-20260717-132501.md
- 2026-07-17 — Distribution LANDED (acceptance item 6 ticked): the npx
  promise is now checked where it can actually break. The smoke packs the
  package as the registry would, installs the tarball into a scratch tree,
  and under **node, bun and deno** starts `serve` on an OS-assigned port
  and drives it over HTTP (`scripts/smoke-serve-assert.mjs`): `/api/health`
  answers, `/` renders the plggmatic ENGINE strip (`pm-row`/`pm-col`/
  `pm-colhead`/`pm-crumbs` — the engine resolved from the registry by a
  REAL consumer, which is the failure `--version` could never see), an
  addressed `/resolve/<doc>` column renders the document, and the server
  is still serving at the end. The qfs-acquisition question the ticket
  left open is answered in **docs/adr/0009**: qfs is FOUND on PATH (or
  named in config), never bundled per-platform à la esbuild and never
  fetched by postinstall — it is the user's credential-holding substrate
  (its store is per MACHINE, `~/.config/qfs`, so a shipped binary would
  not get an empty vault, it would get *theirs* at *our* version), and the
  dependency contract (ADR 0001) forbids the mechanism anyway. A missing
  qfs is a supported state: the boot probe is non-fatal, and one set of
  words (`unreachableAdvice`) names the binary, the install command, and
  the fact that markdown browsing never needed it — asserted by the smoke
  with `bin` pointed at a path that cannot exist, so it holds on every
  machine. Numbered 0009 because unmerged sibling branch
  work-20260717-132501 already claims 0008. check-all green (exit 0) —
  20260717020106-keep-npx-distribution-true-through-the-ui-replacement.md
- 2026-07-17 — **Found, not fixed**: a relocated `serve` outlives its
  launcher. The packed bin relocates out of node_modules and re-execs the
  real server as a CHILD; `kill <the PID the caller started>` leaves it
  serving, reparented to init (measured: launcher 533296 → child 533304,
  port still answering after the kill). So the product's headline command
  starts a server its caller cannot stop. The smoke works around it with
  `pkill -P`; the defect is filed as
  20260717153000-a-relocated-serve-outlives-its-launcher.md, whose test is
  the workaround's removal. Not reproducible from a source checkout —
  relocation is a no-op there — which is why only the packed path sees it.
- 2026-07-17 — story reported — work-20260717-150001.md
