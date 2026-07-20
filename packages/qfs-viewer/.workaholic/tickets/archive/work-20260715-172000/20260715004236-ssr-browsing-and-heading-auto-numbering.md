---
created_at: 2026-07-15T00:42:36+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort:
commit_hash:
category:
depends_on: [20260715004235-markdown-scanner-and-frontmatter-index.md]
mission: build-insightbrowser-on-the-plgg-family
---

# SSR HTML browsing with heading auto numbering, via a new plgg-md heading seam

## Overview

Serve the indexed corpus as server-rendered HTML: a route resolves to a document in the ticket-2 index, renders its markdown, and returns a full page — with **heading auto numbering** (`1-2.`, `3-1-2.`) computed from the document's heading hierarchy. After this ticket, `npx insightbrowser` shows the corpus on `http://localhost:4100/`.

**This ticket carries a cross-repo prerequisite.** Discovery established that heading numbering **cannot** be injected into plgg-md today: `RenderOptions` exposes exactly four seams — `{highlighter, resolveLink, rawHtml, slug}` — and `renderHeading` in `mdToHtml.ts` is `const`, not `export const`, so it is unreachable from outside the package. The chosen route is to **add a heading-decoration seam to plgg-md upstream**, release it, and consume it here as a published `^version` bump. This is the cleanest long-term answer — plggpress benefits too — and it matches the mission's stated contract exactly: *"upstream gaps are filed upstream and consumed here as a published `^version` bump — never vendored, never patched in place, never consumed from a sibling checkout."*

The consequence is real and must be planned for, not discovered: **this ticket blocks on a plgg-md release.** Steps 1–3 happen in the plgg repository; steps 4+ cannot start until the new plgg-md version is on the registry. If the release stalls, the fallback is to render headings locally over the public `parseBlocks` AST (see Considerations) — but that is a deliberate re-decision, not a silent drift.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — applies to all code work.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to all code work; no `any`/`as`/`!`/`@ts-ignore`.
- `workaholic:planning` / `policies/accessibility-first.md` — **the strongest constraint here.** SSR must emit semantic HTML with real `h1`–`h6` preserving hierarchy (never faked with styling), and a **stable per-heading anchor** so a `3-1-2.` section is citable back to page + section. WCAG 2.2 AA is the floor. The same structure is what the MCP server later exposes — the numbering/anchor model is an **AI-reachability** decision, not a cosmetic one.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — the SSR router is a thin `entrypoints/` shell calling the domain's public procedures; it is evidence the separation held that REST and MCP will later start the same procedures identically.
- `workaholic:implementation` / `policies/type-driven-design.md` — the heading tree is a sum type; fold it with `never`-based exhaustiveness.
- `workaholic:implementation` / `policies/functional-programming.md` — rendering and numbering are pure core; the HTTP write is shell.
- `workaholic:design` / `policies/modeless-design.md` — hold navigable state in the URL (current document, filters, open section) — never a server session. This is the practice the mission's column-accretion gate depends on, and it is nearly free for SSR + no-cache.
- `workaholic:design` / `policies/modular-monolith-first.md` — SSR is one surface of one deployment unit.
- `workaholic:implementation` / `policies/objective-documentation.md` — describe numbering as *"an h3 under the second h2 of the third h1 renders `3-2-1.`"*, never *"renders hierarchical numbering"*.
- `workaholic:implementation` / `policies/test.md` — judge SSR by assertions on rendered state, not eyeballed markup.

## Key Files

Upstream (the plgg repository — steps 1–3 edit these):

- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Render/model/seam.ts` (lines ~87–92) - `RenderOptions = { highlighter; resolveLink; rawHtml; slug }` — the complete injectable seam set. **The new heading seam is added here.**
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Render/usecase/mdToHtml.ts` (lines ~249–268) - `const renderHeading = (level, id, children) => ...` — module-private today; it must consult the new seam.
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Render/model/MarkdownDoc.ts` - `MarkdownDoc = { frontmatter; firstHeading; body; links; slugs; headings }` with `MdHeading = { level; text; slug }`. `headings` is in document order and **shares one slugger run with `body`** — the invariant its doc comment protects. The numbers must obey the same invariant.
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Render/usecase/slugify.ts` - `makeSluggers(slug?)`: a per-page closed-over counter behind a pure `next`, constructed fresh per document so state cannot leak. **The exact pattern the heading numberer copies.**
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Block/model/Block.ts` - `Heading = Box<"Heading", { level: HeadingLevel; text: SoftStr }>`, branded `HeadingLevel` 1–6. Level is the only input numbering needs.
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Render/usecase/renderMarkdown.ts` - the three public entries; `renderMarkdownWithOptions(options)(source)` is where InsightBrowser enters.

Reference (do **not** edit):

- `/home/ec2-user/projects/plgg/packages/plggpress/src/router/pressRouter.ts` - the structural template: one generic handler reconstructing the document from the request path, every fs throw lifted to a typed `HttpError`. InsightBrowser resolves against the **in-memory index** instead of `candidateFiles`' fs probe, which removes the `readOne`/`readSource` fallback ladder entirely.
- `/home/ec2-user/projects/plgg/packages/plgg-server/src/View/usecase/response.ts` - `pageResponse<Msg>(opts, status?, headers?)` for a full page.
- `/home/ec2-user/projects/plgg/packages/plgg-server/src/View/usecase/htmlDocument.ts` (lines 15–20) - `HtmlDocumentOptions<Msg> = { title; root; clientEntry? }`; folds `css()` atoms into inlined critical CSS and mounts in `<div id="root">`. `clientEntry?` is a genuine optional under `exactOptionalPropertyTypes` — **omit the key, don't pass `undefined`**.
- `/home/ec2-user/projects/plgg/packages/plggpress/src/framework/Serve/usecase/serveApp.ts` - the persistent-server template: converts Web→Fetch with `toFetch` and resolves once bound so the caller reads the real port and closes deterministically.
- `/home/ec2-user/projects/plgg/packages/plgg-server/src/{node,bun,deno}.ts` - already-shipped multi-runtime entrypoints; the model for the mission's node+bun+deno requirement (that acceptance item is a later ticket, but do not foreclose it here).

Target (this repository):

- `packages/insightbrowser/src/entrypoints/` - the SSR router and the serve entry.
- `packages/insightbrowser/src/domain/usecase/` - the numbering fold (pure).

## Related History

None in this repository. Upstream, `plgg-md`'s `makeSluggers` established the per-document-counter pattern and the "one run, shared by `headings` and `body`" invariant that this ticket's numbering must not break — the single most instructive precedent available.

## Implementation Steps

**Upstream, in `/home/ec2-user/projects/plgg` (a separate branch/PR in that repo):**

1. **Design the heading seam** in `plgg-md`'s `RenderOptions` — e.g. a `decorateHeading` hook receiving the heading's level, text, slug, and its position in the document's heading sequence, returning the phrasing children to render. Keep it optional with a no-op default so every existing consumer (plggpress, plgg-cms) is unaffected.
2. **Thread it through** `renderHeading` in `mdToHtml.ts`, and decide whether `MarkdownDoc.headings`' `MdHeading` gains the computed decoration — if it does, it must come from **one** counter run shared with `body`, exactly as `slugs` are today, or `headings` and `body` will drift.
3. **Test, release, publish** plgg-md to the registry (the plgg repo's own gate and publish path). Record the new version.

**Here, in InsightBrowser (blocked until step 3 lands on the registry):**

4. **Bump** `plgg-md` to the new `^version` in `packages/insightbrowser/package.json`; `./scripts/npm-install.sh`.
5. **Write the numbering fold** (`domain/usecase/`, pure): a per-document counter on the `makeSluggers` pattern — a 6-slot level stack where `next(level)` increments the slot at `level` and truncates deeper slots, emitting `1-2.`, `3-1-2.`. Construct it fresh per document so state cannot leak between documents.
6. **Write the SSR router** (`entrypoints/`, on the `pressRouter` shape): one generic handler that resolves the request path against the **ticket-2 index** (not an fs probe), 404s on a miss as a typed `HttpError`, and holds one consistent index reference for the duration of the read.
7. **Render**: `renderMarkdownWithOptions({ ...seams, decorateHeading: numberer })(source)` → `Result<MarkdownDoc, InvalidError>` → wrap `doc.body` in the page shell → `pageResponse({ title, root })`. Emit real `h1`–`h6` with stable `id` anchors (`accessibility-first`); the number is part of the heading's content, not a CSS `::before` — a CSS-only number is invisible to the MCP surface and to assistive tech alike.
8. **Serve** on the `serveApp` pattern (`toFetch` + plgg-server node serve, resolve once bound). Send `cache-control: no-store, must-revalidate` per the ticket-1 no-cache ADR (precedent: `plgg-poc-portal`/`plgg-poc1-search` `entrypoints/serve.ts`).
9. **Hold navigable state in the URL** (`modeless-design`) — the current document and any open section live in path/query. This is the foundation the column-accretion UI (a later ticket) builds on; do not park it in a session.
10. **Test**: numbering boundaries — skipped levels (`h1` → `h3`), a document with no `h1`, an empty document, a document starting at `h2`, six-deep nesting, and repeated identical heading text (numbers must stay correct while slugs dedup). Assert on rendered state, not eyeballed markup.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `plgg-md` exposes the heading seam, is **published to the npm registry**, and InsightBrowser consumes it as a `^version` dependency — no `file:` dep, no sibling-checkout path, no vendored copy.
- The seam is backward-compatible: with `decorateHeading` omitted, plgg-md's existing rendering output is byte-identical to the prior release (assert in the upstream package's tests).
- An `h3` under the second `h2` of the third `h1` renders the number `3-2-1.` in its heading content.
- Skipped levels behave as specified and documented: an `h1` followed directly by an `h3` produces a defined, tested number — not a crash and not a silent gap.
- Every rendered heading is a real `h1`–`h6` element (never a styled `div`) carrying a stable `id` anchor; the number is in the element's content, not injected by CSS.
- `GET /` on port 4100 returns 200 and lists documents from the index; `GET` a known document path returns 200 with its rendered body; an unknown path returns 404 as a typed error, not a thrown exception.
- Every response carries `cache-control: no-store, must-revalidate`.
- The numbers in `MarkdownDoc.headings` and in `body` agree — they come from one counter run (assert directly).
- `tsc --noEmit` passes; no `any`/`as`/`!`/`@ts-ignore`; `gate-vendor-boundary.sh` confirms the router lives under `entrypoints/`.

**Verification method** — the commands/tests/probes that prove them:

- `./scripts/check-all.sh` green in **both** repos: plgg's gate for the seam, InsightBrowser's for the consumption.
- `npm view plgg-md version` shows the new version on the registry before step 4 starts.
- Unit tests for the numbering fold cover every boundary listed in step 10, asserting emitted numbers as data.
- A live probe with Playwright against `http://localhost:4100/`: load a document with known nesting, assert the `h3`'s text contains `3-2-1.`, assert its `id` anchor is present and stable across a reload, and assert the response header carries `no-store`.
- `grep` the rendered HTML for `<div` masquerading as a heading — none.

**Gate** — what must pass before approval:

- Both repos' gates green, with plgg-md consumed from the registry at its new `^version`.
- The live probe on port 4100 shows correctly numbered, anchored headings in a real browser — this is the mission's `gate_type: live-app` surface, so it is judged live, not from unit tests alone.
- The backward-compatibility assertion for the omitted seam is green upstream.

## Considerations

- **This ticket blocks on a cross-repo release, by design.** Steps 4+ cannot start until the new plgg-md is on the registry, and `/drive` will stall there. Sequence the upstream PR first, or expect the drive to pause mid-ticket. This is the accepted cost of the chosen route; the alternative was rejected knowingly.
- **The fallback, if the release stalls**: render headings locally over plgg-md's **public** `parseBlocks` AST with our own block renderer, threading the counter exactly as `makeSluggers` threads its dedup counter. It owns the heading element and needs no upstream change, at the cost of reimplementing block rendering. A third option — post-processing the `Html` tree with `foldHtml`/`mapHtml` — is available but fragile: it re-derives structure from the tree that the AST already stated. Taking either is a **re-decision to record**, not a silent drift (`/home/ec2-user/projects/plgg/packages/plgg-md/src/Block/usecase/parseBlocks.ts`).
- **The one-counter-run invariant is the trap.** `MarkdownDoc`'s doc comment protects it for slugs because `headings` and `body` share a single slugger run. Numbers must obey the same rule — two independent runs will drift, and the drift will surface as a *citation* bug (the MCP surface citing `3-1-2.` while the page shows `3-2-1.`), long after this ticket (`/home/ec2-user/projects/plgg/packages/plgg-md/src/Render/model/MarkdownDoc.ts`).
- **Numbering is an AI-reachability decision.** The same heading structure the MCP server later exposes is what `accessibility-first` requires for humans. A CSS-only number satisfies neither. Get this right here and the MCP citation model is nearly free.
- **plgg-md's heading seam benefits plggpress too** — the guide and any plggpress site could adopt numbering. Frame the upstream PR that way rather than as an InsightBrowser-specific hook, or it will be reviewed as a foreign requirement.
- **`clientEntry?` is a genuine optional** under `exactOptionalPropertyTypes` — omit the key rather than passing `undefined`, or the SSR page will not typecheck (`/home/ec2-user/projects/plgg/packages/plgg-server/src/View/usecase/htmlDocument.ts` lines 15–20).
- **`pressRouter`'s single-`contentDir` assumption does not hold here** — InsightBrowser resolves against a multi-root index, so route→document resolution is ours to define, including how `.workaholic/`, `docs/`, and `packages/` share one route space without collision (`/home/ec2-user/projects/plgg/packages/plggpress/src/router/pressRouter.ts`).
- **The column-accretion UI is a later ticket**, but step 9's URL-held state is its foundation. Do not foreclose it by parking navigation state anywhere else.

---

**Depends on:** `20260715004235-markdown-scanner-and-frontmatter-index.md`. **Upstream prerequisite:** a plgg-md release carrying the heading seam.

---

## Attempt — 2026-07-15 (night drive)

**Status: BLOCKED. Not started; nothing implemented, nothing stashed.** The blocker was verified against the live registry rather than assumed.

### The blocker, precisely

This ticket's chosen route is *add a heading-decoration seam to plgg-md upstream, release it, consume it here as a `^version` bump*. Three independent facts make that unreachable now:

1. **The consumable plgg-md has no seam to extend, and a smaller surface than this ticket assumes.** `plgg-md@0.0.1` — the only version installable under this environment's `min-release-age=7` control — exposes exactly **two** seams: `Highlighter` and `LinkResolver`. It has no `RenderOptions` record, no `slug` seam, no `rawHtml` seam. The four-seam `RenderOptions` this ticket's Key Files describe is **0.0.2**, read from the monorepo *source*, which is ahead of the registry. `renderHeading` is module-private in both (0 occurrences in the published `.d.ts`), exactly as the ticket predicted.
2. **`plgg-md@0.0.2` is not consumable until 2026-07-16 09:11** — published 2026-07-09, plus the 7-day floor.
3. **A new release does not help tonight either.** Even if the seam were designed, threaded, tested, and published upstream within this drive, `min-release-age=7` would hide it until **2026-07-21 16:45**. The npm-only contract (`docs/adr/0001`) forbids the shortcut — no `file:` link, no sibling checkout, no vendoring — and that contract is the mission's central premise, not a detail to bend at 02:00.

This is the accepted cost of ADR 0001's registry boundary, recorded in `docs/adr/0005-pinned-toolchain-under-min-release-age.md` and flagged in this ticket's own Considerations *before* the drive started ("this ticket blocks on a cross-repo release, by design"). It is a planned consequence meeting its date, not a surprise.

### Why the workarounds were declined

- **Override `min-release-age`** — no. It is a supply-chain security control; turning it off to make a gate go green inverts the reason it exists, and it would be disabled in a config file where nobody would ever see it again.
- **Consume plgg-md from `../plgg`** — forbidden outright by the mission's Scope and ADR 0001.
- **Silently switch to the fallback route** (own renderer over `parseBlocks`) — this was a *considered and rejected* alternative when the ticket was written; taking it unattended would reverse a recorded decision without the developer present. It also needs `parseBlocks` + a slug seam, and 0.0.1's surface is thinner than 0.0.2's, so it is **not** actually unblocked tonight either.

### What unblocks it, in order

1. **2026-07-16 09:11** — `plgg-md@0.0.2` consumable. Ticket 2's front-matter half unblocks; this ticket's *fallback* route becomes possible (0.0.2 has the four-seam `RenderOptions` + `parseBlocks` + `makeSluggers`).
2. **The upstream PR** in `/home/ec2-user/projects/plgg`: add `decorateHeading` to `RenderOptions`, thread it through `renderHeading` in `mdToHtml.ts`, keep it optional with a no-op default so plggpress/plgg-cms are unaffected, and decide whether `MarkdownDoc.headings` carries the computed number — from **one** counter run shared with `body`, or `headings` and `body` will drift.
3. **Publish, then +7 days** before it is consumable here.

**Decision needed from the developer** (do not resolve this unattended): the upstream route costs ~7 days of latency per iteration. If SSR browsing is wanted sooner, the fallback route (own block renderer over the public `parseBlocks`, numbering counter threaded as `makeSluggers` threads its dedup counter) is available from 2026-07-16 and can be upgraded to the seam later. That is a re-decision to record, not a drift.
