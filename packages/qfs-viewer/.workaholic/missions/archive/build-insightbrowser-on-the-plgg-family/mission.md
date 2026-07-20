---
type: Mission
title: Build InsightBrowser on the plgg family
slug: build-insightbrowser-on-the-plgg-family
status: abandoned
created_at: 2026-07-15T00:32:03+09:00
author: a@qmu.jp
assignee: a@qmu.jp
tickets: []
stories: []
concerns: []
gate_type: live-app
gate_target: http://localhost:4100/
gate_assert: The page lists markdown documents scanned from the working tree and faceted by tag group; following a document link opens it in a new column to the right without discarding the previous one, and the URL records the traversal so a reload restores the same columns.
---

# Build InsightBrowser on the plgg family

## Goal

A repository's knowledge does not live in one place. It is scattered across `.workaholic/` (tickets, stories, missions, concerns, specs, terms), `docs/`, and per-package `packages/*/README.md` — markdown that is written constantly and read almost never, because nothing makes the corpus navigable as a whole. The filesystem is a tree; the knowledge is not. A ticket is *about* a package, *of* a kind, *from* a mission, and the tree can express only one of those.

**InsightBrowser makes that corpus browsable, editable, and reachable by AI — from one indexed model, served three ways.** It scans the markdown under the directory it is run from, indexes the front matter in memory, and serves it as **SSR HTML** for people, a **REST API** for programs, and an **MCP server** for agents. The same model backs all three, so what a developer reads on screen, a script fetches, and an agent queries over MCP are the same documents with the same tags.

Two users define the product:

- **The local developer** runs `npx insightbrowser` at a repository root during workaholic development. Markdown is scanned at boot, front matter is indexed on memory and hot-reloads on edit, and documents are browsable *and editable* in place — no build step, no database, no central configuration.
- **The hosted reader** reaches the same SSR server deployed to Cloudflare Worker + D1 or Lambda + EFS + sqlite, serving document data that was compressed, optimized, and RAG-indexed ahead of time. The architecture is adaptive to qmu.app: configuration and documents are offloaded to R2, and **nothing is cached** — a stale document is an incident, not a performance win.

Three commitments shape how it is built:

- **Tag groups make the corpus non-tree.** A tag group declares its variations — ticket kind (feature / bugfix / refactor), activity time (code reading / design / implementation / bugfix) — and documents are tagged in front matter. Navigation is by dimension, not by directory.
- **AI reaches it without force.** The REST API and MCP server replace the qmu-co-jp → workaholic *sync* with a live MCP *reference*; browser AI reaches the same corpus over WebMCP; and an in-page Realtime API answers questions about — and edits — the open document by voice, given an `OPENAI_API_KEY`. Loading a plugin exposes that plugin's documentation through MCP, which is how the plgg and qfs docs sites are built on this same mechanism.
- **Traversal stays legible.** A page link resolves *sideways* into a new column rather than replacing the screen, so how you arrived is readable both on screen and in the URL. This follows the **idea** plggmatic proposed — columns as projected depth, the whole navigable state in the URL — but InsightBrowser implements it itself on `plgg-view`. plggmatic is a reference to learn from, **not a dependency**.

InsightBrowser is a **standalone repository consuming the plgg family from npm** — `plgg`, `plgg-view`, `plgg-md`, `plggpress`, `plgg-cms` as published `^version` dependencies, the same cross-repo contract plggmatic uses — and takes **no other dependency**. It is the first real product assembled on the stack the `plggpress-technical-confidence-poc-portal` mission (in the plgg repo) is proving PoC by PoC; verdicts flow in from there, they are not re-litigated here.

## Scope

**Definition of done:**

- `npx insightbrowser` at a repository root scans the markdown beneath it, indexes front matter on memory, hot-reloads on edit, and serves SSR HTML, a REST API, and an MCP server from that one model — on node, bun, and deno.
- The corpus is navigable by tag group, editable in the browser, traversable in accreting columns whose path is legible in the URL, and reachable by AI over MCP/WebMCP and by voice over the Realtime API.
- Principals are managed: users and bots (API-key issued), with RBAC over read and edit.
- The same server is deployable as a hosted SSR target (Cloudflare Worker + D1, Lambda + EFS + sqlite) with pre-optimized RAG-indexed data, R2-offloaded config and documents, and no caching.
- The plgg and qfs documentation sites are built on this mechanism, each plugin's docs loaded through MCP, with arbitrary structure declarable in a config file.
- Every package follows the workaholic directory layout, consumes only the plgg family from npm, and passes one reproducible `scripts/check-all.sh` gate.

**Out of scope:**

- Proving the underlying techniques. The browser search core, reader agent, voice assistant, agent file-editing with hot reload, config generation, and non-tree classification are being proven by `plggpress-technical-confidence-poc-portal` in the plgg repo. This mission **consumes** those verdicts.
- Changing plggpress / plgg-cms internals. Upstream gaps are filed upstream and consumed here as a published `^version` bump — never vendored, never patched in place, never consumed from a sibling checkout.
- Depending on plggmatic, or porting its code. Its column model and scheduler are read as a **design reference**; the UI here is built on `plgg-view` directly.
- Any non-plgg runtime dependency.
- Migrating qmu-co-jp off its sync onto the MCP reference — this mission ships the reference; the cutover is that repo's work.

## Acceptance

<!-- Ticket filenames are attached as (#<ticket>.md) markers when each ticket is filed via /ticket. -->

- [x] Repository skeleton: workaholic directory layout, the npm-only plgg-family dependency contract, and one reproducible `scripts/check-all.sh` gate (#20260715004234-repository-skeleton-and-dependency-contract.md)
- [x] Scanner + on-memory front-matter index over the working tree (`.workaholic/`, `docs/`, `packages/`), hot-reloading on edit (#20260715004235-markdown-scanner-and-frontmatter-index.md)
- [x] SSR HTML browsing: documents render server-side with heading auto numbering (1-2., 3-1-2.) (#20260715004236-ssr-browsing-and-heading-auto-numbering.md)
- [x] Column-accretion UI on plgg-view (plggmatic's idea, implemented here): page links resolve sideways, traversal legible on screen and in the URL
- [x] Tag groups: front-matter tagging with declared groups and variations; the corpus faceted by dimension rather than tree — discovered by default, DECLARED via `insightbrowser.config.json` (order, label, fixed variations, `hide`); discovery stays the floor so a corpus with no config is still navigable
- [x] REST API serving the same indexed model (#20260715014949-rest-api-over-the-index.md)
- [x] MCP server serving the same indexed model — read-only until principals exist; per-plugin documentation loading rides with the configuration item
- [x] `npx insightbrowser` runs from a repository root on node, bun, and deno — `scripts/smoke-npx.sh` packs, installs and RUNS the bin under each, and prints PASS for all three
- [x] In-browser editing writes back to the working tree with the index hot-reloading
- [x] RBAC and principal management: users and bots (API-key issued), enforced over read and edit — declared in `insightbrowser.config.json`, OPEN when none are declared (the `npx` case)
- [ ] Browser AI over WebMCP against the live corpus
- [ ] Voice Q&A and voice editing over the Realtime API with `OPENAI_API_KEY`
- [x] Arbitrary structured configuration file drives layout and classification — `insightbrowser.config.json`, optional; JSON not TS because this runs at repository roots with no toolchain
- [ ] Hosted SSR deployment: Cloudflare Worker + D1 and/or Lambda + EFS + sqlite, with pre-optimized RAG-indexed document data
- [ ] qmu.app-adaptive architecture: config and documents offloaded to R2, no caching, verified
- [ ] The plgg and qfs documentation sites are built and served on this mechanism
- [x] Other qfs resources browsable alongside markdown — declared in `insightbrowser.config.json`, rendered as the table qfs answers with, in the same trail as documents

## Changelog

<!-- Append-only, dated timeline relating this mission's tickets and reports over time.
     One line per event ("- YYYY-MM-DD — event — filename"); never rewrite past lines. -->
- 2026-07-15 — mission created — mission.md
- 2026-07-15 — kickoff ticket filed — 20260715004234-repository-skeleton-and-dependency-contract.md
- 2026-07-15 — kickoff ticket filed — 20260715004235-markdown-scanner-and-frontmatter-index.md
- 2026-07-15 — kickoff ticket filed — 20260715004236-ssr-browsing-and-heading-auto-numbering.md
- 2026-07-15 — decision: published package is `insightbrowser` (npm rejects uppercase names; `npx InsightBrowser` cannot resolve) — 20260715004234-repository-skeleton-and-dependency-contract.md
- 2026-07-15 — decision: heading auto numbering lands via a new plgg-md heading seam upstream, consumed as a published ^version bump — 20260715004236-ssr-browsing-and-heading-auto-numbering.md
- 2026-07-15 — ticket archived — 20260715004234-repository-skeleton-and-dependency-contract.md
- 2026-07-15 — night drive: skeleton implemented; gates proven red-on-violation then green (611f685)
- 2026-07-15 — scanner/index/reload implemented and green; ticket 2 left open — front-matter half blocked (580a157)
- 2026-07-15 — BLOCKED: ~/.npmrc min-release-age=7 hides the plgg family's 07-09..07-13 release burst. plgg-md 0.0.2 (front matter as YamlMap) consumable 2026-07-16 09:11; plgg-cms 0.0.2 2026-07-17; plgg-bundle 0.0.6 + plggpress 0.0.4 2026-07-20 — 20260715004235-markdown-scanner-and-frontmatter-index.md
- 2026-07-15 — BLOCKED: ticket 3 not started — its upstream plgg-md heading seam would be hidden 7 days after publish (2026-07-21); the control was not overridden — 20260715004236-ssr-browsing-and-heading-auto-numbering.md
- 2026-07-15 — ticket archived — 20260715014949-rest-api-over-the-index.md
- 2026-07-15 — REST API implemented and judged live on port 4100 over this repo's own corpus (f5c32d0); a live curl found 404s missing the ADR-0003 no-store header, now fixed and pinned — 20260715014949-rest-api-over-the-index.md
- 2026-07-15 — hot reload wired and judged live: create/edit/delete tracked on port 4100; a 5-write burst coalesced into ONE reload. The 'hot-reloading on edit' half of the scanner item is DONE; the item stays unchecked because its front-matter half is blocked (10e61f4)
- 2026-07-15 — upstream gap filed: plgg-md's YAML subset rejected every workaholic ticket (472/661 of plgg's own corpus) and `renderHeading` was unreachable — plgg 20260715180322-widen-yaml-subset-and-heading-seam.md
- 2026-07-15 — plgg-md 0.0.3 shipped BOTH asks (flow sequences + empty values; the `decorateHeading` seam) with the fail-closed half intact; rejections fell 7/28 → 1/28 here and 472/661 → 2/1711 in plgg — 20260715004235-markdown-scanner-and-frontmatter-index.md
- 2026-07-15 — front-matter index DONE: Option<YamlMap> projected, query surface named after plgg-cms/content/Query, `?mission=…` returns this mission's 5 tickets live (8c0d48c, c2c6213) — 20260715004235-markdown-scanner-and-frontmatter-index.md
- 2026-07-15 — bug found live: the server reloaded forever on its own log (115 reloads in 4s) — `debouncedReload` swapped unconditionally though `applyChange` returns its input for non-documents; plgg had the same shape once (28dbdcf)
- 2026-07-15 — decision: a declined fence indexes with `None` front matter rather than dropping the document — the corpus stayed whole while the upstream gap was open, and healed on a version bump alone — 20260715004235-markdown-scanner-and-frontmatter-index.md
- 2026-07-15 — SSR DONE: documents render with headings numbered from their own outline (`3-2-1.`), number in the element's content not CSS; ticket step 5's self-built counter deliberately NOT written — plgg-md's seam hands the ordinal over already computed, so the 'one counter run' criterion is met by having no counter (b4e9289) — 20260715004236-ssr-browsing-and-heading-auto-numbering.md
- 2026-07-15 — decision: a skipped heading level keeps its zero (`1-0-1.`); dropping it would collide with a real h2's `1-1.` and make two positions cite identically — 20260715004236-ssr-browsing-and-heading-auto-numbering.md
- 2026-07-15 — dependency diamond resolved: plgg-md@0.0.3 needs plgg-view ^0.0.2 (Html gained `Raw`) while plgg-server@0.0.3 pinned ^0.0.1; plgg-server 0.0.4 (published 07-09) fixes it. An earlier session note claiming plgg-view could not move was wrong — it never checked the constraining package (1213d94)
- 2026-07-15 — column accretion DONE and the mission's gate met live: a link inside a document opens the next column, the previous survives, and `?cols=a.md,b.md` restores the same screen on reload. Server-rendered with NO client JavaScript — which is what makes the URL the state rather than a report of it
- 2026-07-15 — the trick: plgg-md's `resolveLink` seam rewrites in-document links per column, so the same file at a different depth resolves its links differently. The document's markdown is untouched
- 2026-07-15 — bug found by driving the real corpus: `docs/adr/index.md` writes `](0001-x.md)` meaning its NEIGHBOUR, and resolving it root-relative pointed every ADR-index link at a nonexistent document. `resolveRelativePath` now resolves against the containing document
- 2026-07-15 — tag groups DONE: dimensions discovered from the corpus's own front matter (`layer`, `type`, `mission`), a sequence putting one document on several variations of one dimension — the thing a directory cannot do
- 2026-07-15 — regression caught by an old spec: the column view dropped the home page's scan-error section. 'GET / reports scan errors rather than hiding them' survived the rewrite and failed correctly; the section is back
- 2026-07-15 — MCP DONE and driven live with real JSON-RPC frames: initialize / tools/list / tools/call over stdio, exposing list_documents, get_document, list_tag_groups, corpus_health
- 2026-07-15 — the mission's central claim is now demonstrable: `?mission=build-insightbrowser-on-the-plgg-family` over HTTP and `list_documents {"mission": "…"}` over MCP both answer the SAME 5 tickets, because both call listCollection over one Index. Nothing re-implements the query
- 2026-07-15 — found: `plgg-mcp` 0.0.1 (qmu, published 07-09) is a hand-rolled JSON-RPC 2.0/MCP on plgg primitives with no @modelcontextprotocol/sdk — so MCP lands without breaking ADR 0001 and without hand-rolling a protocol (which ADR 0006 rejected for OTLP)
- 2026-07-15 — decision: the MCP surface is READ-ONLY. An MCP tool that writes is an unauthenticated write to the working tree until the RBAC/principals item exists
- 2026-07-15 — in-browser editing DONE and driven live: GET /edit/<path> → textarea → POST → 303 → the file on disk changed and the index already knew. A plain form, no client JavaScript
- 2026-07-15 — decision: the WRITE seam is separate from FileSystem. The scan, index, watcher and every query are readers; folding writeFile into FileSystem would hand write authority to the walk, which touches every file in the repository
- 2026-07-15 — decision: the editor applies its own edit to the index rather than waiting for the watcher. The watcher debounces 50ms and the 303 returns immediately, so the follow-up GET would render the OLD source and the author would watch their save not happen
- 2026-07-15 — bug found, same shape as the 404 one: a POST to a read-only server returned `MethodNotAllowed` as an Err that never passed through the no-store middleware — the ADR 0003 escape again, wearing a different method. A POST catch-all closes it
- 2026-07-15 — REALITY CHECK on the remaining items. #12 voice needs OPENAI_API_KEY (absent), #14/#15 hosted+R2 need wrangler and a Cloudflare account (absent), #16/#17 need changes to the plgg and qfs repositories (only reachable via /request). None can be met or gated from this environment
- 2026-07-15 — #8 (node/bun/deno) is NOT met and must not be checked: bun cannot run the product — the self-alias needs node's `register()` hook. Ticket 20260715223000 carries the fix. deno's installer was refused here, so even node+bun is not the whole item
- 2026-07-15 — configuration DONE and driven live: `insightbrowser.config.json` sets the title, orders and labels the facets, fixes a taxonomy's variations, and hides keys. Verified against a real file: title → 'Our Knowledge', 'Kind' leading, `author` gone, and a declared `refactor (0)` showing as an empty shelf
- 2026-07-15 — decision: JSON, not a `.config.ts` like plggpress. plggpress's consumers are TypeScript projects with a toolchain; this runs at ANY repository root, so a config that must be compiled would exclude most of the corpora it exists to browse
- 2026-07-15 — decision: a config REFINES, never replaces. A declared group leads; every undeclared key the corpus carries still appears after it, because the keys nobody thought to declare are the ones worth seeing. Hiding takes an explicit `hide`
- 2026-07-15 — decision: a malformed config STOPS the boot. Starting with the default would discard what someone wrote on purpose in silence, and they would find out by noticing their facets were wrong
- 2026-07-15 — bug found live: a malformed config reported the error and then exited 0. `serveCorpus` set `process.exitCode = 1` but `cli.ts` did `process.exitCode = main()`, and main returned 0 for `serve` unconditionally — CI would have read the failed boot as a success
- 2026-07-15 — bun RUNS the product. My 'the alias needs node's register() hook, so bundle the CLI' diagnosis was wrong: `tsconfig.json` simply did not ship (files was [dist,src,bin]) and relocate copied only package.json — bun/deno read the paths from there. Second cause: a static plgg-mcp import loaded node:sqlite, which bun lacks; the MCP import is lazy now (10ffdaa)
- 2026-07-15 — the 'SQLite warning' ticket under-sold its own defect: on node it was noise, on bun it was fatal. Same bug, different runtime
- 2026-07-15 — the smoke check ran node ONLY, which is how bun stayed broken through a session of green gates. It now runs every runtime it finds and prints a loud SKIP for the rest
- 2026-07-15 — MECHANISM PROVEN on the other corpora WITHOUT touching them: plgg's 1711 documents indexed in 249ms with facets; qfs's 577 the same. Item #16 still needs those repos to ADOPT it, which is their work — but the readiness is no longer a claim
- 2026-07-15 — RBAC DONE and driven live: no token → 401, wrong key → 401, reader → 200, reader writing → 403, editor writing → 303, and the 401 carries no-store like everything else
- 2026-07-15 — decision: principals are DECLARED in the config, not stored. plgg-auth is an OIDC identity PROVIDER and drags plgg-sql + plgg-db-migration — a database, which the mission's Goal rules out for the local surface
- 2026-07-15 — decision: OPEN when no principals are declared. `npx insightbrowser` at your own repo, on your own machine, reading your own files needs no token; demanding one is theatre for an audience of one. Access control turns on when a repository declares it stopped being one developer's tree
- 2026-07-15 — decision: enforcement is a MIDDLEWARE over every route and derives write-ness from the METHOD, so a route added later cannot forget to be covered. A per-handler check is a thing you can forget, and the one you forget is the hole
- 2026-07-15 — qfs resources DONE and driven live: `?cols=docs/adr/index.md,res:repo-files` opens a rendered document and a live qfs table side by side, with qfs's reported column types on the headers
- 2026-07-15 — the design question answered by READING what qfs returns, not by guessing: `{schema, rows, meta}` is a TABLE, so a resource is NOT a Document. Projecting it into one would have meant rendering it to prose (losing the schema, lying about authorship) or widening Document until it meant anything. `Stop` is a tagged union of Doc | Resource, and the trail carries both
- 2026-07-15 — decision: resources are DECLARED, never discovered. qfs reaches mail, databases and cloud accounts; enumerating it onto a page would make a knowledge browser an exfiltration tool its own repository never asked for
- 2026-07-15 — decision: fetched per request, never indexed. A live table's value is being live; caching it would make it a stale copy of what qfs already holds (docs/adr/0003)
- 2026-07-15 — my first qfs timeout (6s) was a guess and it timed out on a directory listing. MEASURED: a trivial local select takes 7.2s because qfs registers drivers on every invocation. 20s now, and the comment says the real fix is a qfs daemon rather than a bigger number
- 2026-07-16 — #8 (node/bun/deno) IS met, and only running it could say so. The carry ticket predicted "the tsconfig fix SHOULD cover deno for the same reason it covered bun" — it does not. deno reads `deno.json`, not `tsconfig.json`, and its `node:module` `register()` is a silent no-op stub (it accepts a hook path that does not exist without throwing), so deno had no self-alias resolver at all and fell through to `exports`
- 2026-07-16 — the root cause was that ONE fact was declared THREE times: `bin/hook.mjs` for node, tsconfig `paths` for bun, and nothing for deno. "The alias resolves" was true once per runtime instead of true once. It is `#insightbrowser/*` in package.json's `imports` now — the one mechanism node, bun, deno and tsc all implement — and `hook.mjs` is deleted. A `#` specifier is in-package by construction, so the vendor-boundary gate classifies on the sigil rather than on a name any third party could take
- 2026-07-16 — the npx smoke was SILENT on the runtime that failed: `ACTUAL=$(... 2>/dev/null)` under `sh -eu` discarded the error and exited at the assignment, before its own FAIL line. The check that exists to prove every runtime runs the bin said nothing about the one that could not. Fixed first, before the thing it measures
- 2026-07-16 — `src/domain/model/Index.ts` held a literal NUL byte (the `errorKey` separator), which made it `data` to file(1) and BINARY to grep — so it answered "no match" to every search while looking normal on screen, and a package-wide rename silently skipped it. Written `\0` now: same string, same behaviour, greppable file. Twice today a gate caught what my own confident measurement missed
- 2026-07-16 — **WebMCP is NOT blocked and never was.** The item was ruled out because the npm `webmcp` package is a third party's, so ADR 0001 excluded it. All true, and beside the point: WebMCP is a **browser platform API** (`navigator.modelContext.registerTool`) that needs NO dependency, so ADR 0001 never reached it. The registry was the constrained thing; the browser was the constraining one — the fourth time on this branch that "impossible" was an unfinished diagnosis, and the first that survived a whole session because it *looked* well-evidenced (an npm check, a maintainer name, an ADR citation)
- 2026-07-16 — MEASURED: Chromium **149** (`~/.cache/ms-playwright/chromium-1228`) exposes `navigator.modelContext` unflagged — `ModelContext` with `registerTool`/`getTools`/`ontoolchange`; Chromium **147** does not, which makes it a free negative control. WebMCP is a W3C Web ML CG **Draft Community Group Report** (2026-07-10), not a Standard — and the spec already says `document.modelContext` while the shipped browser says `navigator.`, six days apart. A January-cutoff model cannot reason about a July web platform; it has to look. Ticket `20260716025007`
- 2026-07-16 — the open question WebMCP forces is not availability but the stance: this product ships **no client JavaScript**, and that is what makes "every column is a function of the URL" true. ADR 0007 decides it. The distinction to hold: the claim is *works with no JS*, not *ships no JS* — so an additive, non-rendering script keeps it and client-side rendering or client-held state would not. Also corrected: `plgg-server` exports no `clientEntry` (it is an optional field on `HtmlDocumentOptions`, paired with the `javascriptResponse` export)
- 2026-07-16 — **Hosted SSR was not blocked on credentials either.** `aws sts get-caller-identity` answering `NoCredentials` was real and asked the DEFAULT profile; profile `q` authenticates with PowerUserAccess on account `839625015061` (developer-named and authorised). The `and/or` means the Lambda half alone satisfies the item, so Cloudflare's absence blocks nothing. Fifth wall to fall on this branch, and the second in a row where the command was real but aimed at the wrong artefact — the default profile, the npm registry. Ticket `20260716093913`; nothing is provisioned before ADR 0008 and a written teardown, because it is a live corporate account
- 2026-07-16 — **the R2 item was mis-filed as "the same Cloudflare wall".** An account is necessary and not sufficient: `FileSystem` is SYNCHRONOUS (`readdirSync`/`statSync`/`readFileSync`), so EFS works over it unchanged because EFS is POSIX — while R2 and S3 are object stores that a sync seam cannot express at all. That row needs `FileSystem` to become async, reaching `scan`, `reload`, the index and every caller. A domain change wearing an infrastructure hat, and the sync seam is also the honest argument FOR EFS rather than "the mission said EFS"
- 2026-07-16 — a trap noted while ticketing: the hosted item says "with pre-optimized **RAG-indexed** document data", a phrase never defined. RAG normally means embeddings, embeddings mean a provider, and `printenv OPENAI_API_KEY` is empty — so that clause may be blocked while the hosting is not, or may mean something cheaper needing no provider. Get it defined rather than building a pipeline to satisfy a phrase
- 2026-07-16 — **`.worktrees` was never pruned, so "plgg's 1711 documents" (above, 2026-07-15) was 44% duplicates.** Measured with the prune: plgg is **954 documents in 186ms**, qfs **578 in 191ms** — and the scan is 8× faster because it stopped reading the same files three times. The errors fell 2→1 on both: the "second" error was one file seen through a worktree. A git worktree is a transient checkout of ANOTHER branch, so its documents are this corpus's documents at a different commit — copies, not knowledge, and a reader searching a phrase got it back three times with no way to tell which was live
- 2026-07-16 — the sting in it: a worktree is not an exotic layout here, it is **the method** — every `/trip` and parallel `/drive` makes one — so a tool for browsing workaholic repositories duplicated exactly the repositories it exists for, and worst on the biggest. It never showed here because this repo is served FROM inside a worktree, which has no nested `.worktrees/` to trip over. The one number nobody re-derived was the one that was wrong
- 2026-07-16 — and the old ticket's `qfs (577)` was RIGHT while my "1154" was the bug: qfs had no worktree when it was measured and has one now. I was one step from filing a `/request` advertising an inflated count to the maintainers of the corpus it described. Re-deriving a number is not the same as trusting a fresh one — check what it counted
- 2026-07-16 — **the docs-sites item does not need plgg or qfs to adopt anything.** Running the tool at a directory is reading it, so we SERVE them: `workloads/docs/` + `scripts/serve-docs.sh <path>`, published at `plgg-docs.qmu.dev` → `localhost:4101` (954 documents, read-only, behind Access). I nearly filed a `/request` instead and stopped one step from writing into two PUBLIC repos asking them to `npx insightbrowser` — which 404s, because the package is unpublished and this repo is PRIVATE. That request would have disclosed a private product to the world and asked for something nobody could do
- 2026-07-16 — decision: `serve --read-only` withholds the writer rather than checking for it, so `/edit` returns **404 not 403** — the route does not exist because the capability was never granted. `serve` had always granted write authority, and pointed at plgg that is a browser with commit rights over someone else's tree; `guard-repo-confinement.sh` cannot see a write that comes from a browser. The composition root had already written the shape ("a hosted deployment builds its app without it and is read-only by construction") — there was simply no way to ask for it
- 2026-07-16 — the tunnel's documented health check is WRONG in the same way the 302 rule was: `grep "Registered tunnel connection" <log>` waited forever on a healthy tunnel that wrote nothing to that file, and would "pass" on yesterday's lines for a tunnel that is down now. `cloudflared tunnel info qmu-dev` asks Cloudflare — the EDGE column summing to four, with a CREATED matching this run. Ask the thing itself
- 2026-07-16 — the docs surface is deliberately NOT the whole item yet: qfs is unpublished (one `WORKAHOLIC_DOCS_PORT`, so one corpus at a time), and it serves the LIVE working tree — plgg sits on `work-20260716-023712`, **not pushed** — so it publishes unshared WIP to everyone behind Access. Tolerable for internal review, not for a public docs site, which should serve `origin/main` from a dedicated checkout
- 2026-07-16 — abandoned because the qfs-viewer MVP plan reverses its commitments (plggmatic engine becomes the UI, qfs becomes required, markdown scanning moves to a qfs collection path); code assets remain on main for selective reuse during the repurpose — mission.md
- 2026-07-16 — mission abandoned — mission.md
