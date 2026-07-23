---
created_at: 2026-07-15T00:42:35+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on: [20260715004234-repository-skeleton-and-dependency-contract.md]
mission: build-insightbrowser-on-the-plgg-family
---

# Markdown scanner and the on-memory front-matter index, hot-reloading

## Overview

Scan the markdown beneath the directory InsightBrowser is run from — `.workaholic/`, `docs/`, `packages/` — parse each file's front matter, and hold the result as an **immutable on-memory index** that hot-reloads when a file changes. This is the mission's one model; the SSR, REST, and MCP surfaces are all thin entry points over it.

Discovery established two things that decide the ticket's shape:

- **Front-matter parsing is a solved problem upstream.** `plgg-md`'s `parseFrontmatter(source: SoftStr): Result<ParsedDocument, InvalidError>` is total and never throws. No hand-rolled YAML.
- **Hot reload is genuinely new code.** There is **no** `fs.watch`, chokidar, or any watcher anywhere in the plgg family. plggpress's dev reload works by plgg-bundle re-importing `devEntry` with a busted module version — and plggpress's own doc comment says its `serve` mode "loads config ONCE at startup and never watches". `npx insightbrowser` **is** serve mode. This ticket owns the watcher outright, including index-swap semantics under in-flight reads.

The closest scanner precedent, `plgg-server`'s `discoverPaths`, is single-root and route-oriented (it yields route paths and collapses `foo.md` and `foo/index.md` onto `/foo/`). InsightBrowser needs multi-root **document records**, so it is a model to adapt, not an API to call.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — applies to all code work.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to all code work; no `any`/`as`/`!`/`@ts-ignore`.
- `workaholic:implementation` / `policies/type-driven-design.md` — **the** governing policy here. The scanner is an input boundary: front matter arrives as `unknown` and must pass `asXxx(unknown) => Result<Xxx, InvalidError>` before flowing inward — never `Record<string, string>`. Brand the same-shape/different-meaning strings (document slug vs file path vs heading anchor vs route).
- `workaholic:implementation` / `policies/functional-programming.md` — shell/core split: the fs walk and the watcher are shell; front-matter projection, index build, and query are pure core. Hot reload **generates a new immutable index value**; it never mutates. The watcher and clock are passed as arguments so reload is testable without a real watcher.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — `node:fs` and the watcher live under `vendors/`; the index and its query surface are domain.
- `workaholic:implementation` / `policies/test.md` — at least one 3–5 line unit test per public function. Test against the real thing: the scanner runs over **real fixture trees the test itself generates** — no shared dataset, because it destroys reproducibility.
- `workaholic:implementation` / `policies/observability.md` — instrument the scanner from creation, not later: structured JSON logs (`Indexed 47 files` via `console.log` is the named anti-pattern), finite timeouts, and bounded retries so a malformed or vanishing file cannot take the index down during reload.
- `workaholic:planning` / `policies/terminology.md` — reuse the glossary fixed in ticket 1 (*document*, *front matter*, *index*, *scan*); align the query vocabulary with `plgg-cms/src/content/Query` rather than inventing a parallel one.
- `workaholic:design` / `policies/sacrificial-architecture.md` — do **not** pre-optimize the on-memory index.

## Key Files

Reference (do **not** edit — other repositories):

- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Frontmatter/usecase/parseFrontmatter.ts` - **the primary API this ticket consumes.** `parseFrontmatter(source: SoftStr): Result<ParsedDocument, InvalidError>`, `ParsedDocument = { frontmatter, body }`. Total, never throws. A fence-less file returns `Ok` with `None` data (so un-front-mattered markdown under `packages/` indexes cleanly); an unterminated or malformed fence is a **positioned `Err`** the index must skip-and-report rather than fail boot on.
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Frontmatter/model/Frontmatter.ts` - `Frontmatter = { layout: Option<SoftStr>; data: Option<YamlMap> }`. `layoutOf` derives `layout` from the data map — the precedent for deriving tag-group fields from `YamlMap` rather than adding parallel parsing.
- `/home/ec2-user/projects/plgg/packages/plgg-md/src/Yaml/model/YamlValue.ts` - **the normative front-matter grammar bound, and the single biggest constraint on this ticket.** `YamlMap = ReadonlyArray<[SoftStr, YValue]>`; `YValue` is a scalar, a `YSeq` of scalars, or a **one-level** `YMap` of scalars — nothing deeper. Anchors, tags, block scalars, and flow collections are parse errors; duplicate keys are an error (not last-wins); dates must be quoted strings.
- `/home/ec2-user/projects/plgg/packages/plgg-server/src/Ssg/usecase/writeStatic.ts` - `discoverPaths(rootDir): PromisedResult<ReadonlyArray<SoftStr>, SsgError>` (~line 337): a `tryCatch`-wrapped recursive walk with a skip list. Single-root and route-oriented — **adapt, don't call**.
- `/home/ec2-user/projects/plgg/packages/plgg-cms/src/content/Query` - the existing content-index vocabulary to rhyme with: `model/{CollectionSchema,ListQuery,ListResult}.ts`, `usecase/{registerCollection,listCollection,getDocument,searchIndex}.ts`. SQL-backed there, on-memory here; the query **shape** should match.
- `/home/ec2-user/projects/plgg/packages/plggpress/src/devEntry.ts` - the only reload precedent, and it is **not** a watcher: `pressDevEntry` is re-invoked by the plgg-bundle dev toolchain on each edit. Its doc comment names the three modes; `serve` never watches. Read it to understand why it does not apply here.

Target (created by ticket 1):

- `packages/insightbrowser/src/domain/model/` - the document record, the index, the branded types, the `asXxx` casters.
- `packages/insightbrowser/src/domain/usecase/` - the pure index build, projection, and query.
- `packages/insightbrowser/src/vendors/` - `node:fs` walk and the watcher.

## Related History

None in this repository — this is the second implementation ticket. The relevant history is upstream: plgg's `plgg-cms` content-index work established the `ListQuery`/`ListResult`/`getDocument` vocabulary this index should rhyme with, and the `plggpress-technical-confidence-poc-portal` mission (plgg repo) is proving the browser-side search core that will eventually query this index.

## Implementation Steps

1. **Model the document record** (`domain/model/`): repo-relative path (branded), the parsed front matter, and the derived tag-group fields. Brand slug vs path vs anchor vs route so they cannot be interchanged.
2. **Write the casters** (`type-driven-design`): `asDocument(unknown) => Result<Document, InvalidError>` and the tag-group casters, projecting `Option<YamlMap>` inward. Match exhaustively over `YValue`'s five patterns (`yStr$`/`yNum$`/`yBool$`/`ySeq$`/`yMap$`) via `matchOption` over the map — mirroring how `layoutOf` derives `layout`.
3. **Write the multi-root walk** (`vendors/`, on the `discoverPaths` model): a `tryCatch`-wrapped recursive walk with a skip list (`node_modules`, `dist`, `.git`, `outputs`), fanning across the three roots where `discoverPaths` takes one, yielding **document records** rather than routes. Do not collapse `foo.md` and `foo/index.md`.
4. **Build the index** (`domain/usecase/`, pure): fold the records into an immutable index keyed by repo-relative path, with reverse maps per tag dimension. On a per-file `Err`, **skip-and-collect** — a malformed fence in one `packages/` file must not take the server down — and report the collected errors as structured data.
5. **Write the query surface**, naming it after `plgg-cms/src/content/Query` (`ListQuery`/`ListResult`/`getDocument`/`listCollection`). Pure functions over the index value.
6. **Write the watcher** (`vendors/`, NEW — no precedent): `node:fs` watch over the three roots, debounced, re-parsing only the changed file. Pass the watcher and clock as arguments so reload is testable without a real watcher.
7. **Define the swap semantics explicitly**: reload builds a **new** index value and swaps the reference atomically. State — and test — what an in-flight SSR read observes during a swap (it must see one consistent index, never a torn one). This is the ticket's subtlest requirement.
8. **Instrument** (`observability`): structured JSON logs for scan start/complete/error with counts and durations; finite timeouts and bounded retries around fs reads so a vanishing file during reload is a logged skip, not a crash.
9. **Test** (`test`): each test generates its own fixture tree. Cover the boundaries — a fence-less file, an unterminated fence, a malformed fence, duplicate keys, a deeper-than-one-level map (a `YamlMap` parse error), an empty root, a file deleted mid-scan, and a swap observed by a concurrent read.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Scanning a generated fixture tree yields one index record per markdown file **anywhere in it** — including root-level `README.md`/`CLAUDE.md` — keyed by repo-relative path, with front-matter fields projected into typed values (not `Record<string, string>`). (Amended 2026-07-15: the original said "all three roots (`.workaholic/`, `docs/`, `packages/`)", which read the mission's examples — 「…などにも散らばる」 — as an exhaustive list and left this repository's own README out of its corpus. The scan is now the whole tree minus `PRUNED_DIRECTORIES`.)
- A file with **no** front-matter fence indexes cleanly as a document with no front-matter data (`parseFrontmatter` returns `Ok`/`None`) — it is not an error.
- A file with a **malformed or unterminated** fence is skipped and reported in the index's collected errors; the scan still completes and every other document is indexed. The process does not exit.
- Editing a watched file updates that document's index entry within the debounce window **without** re-scanning the whole tree, and produces a **new** index value (the previous value is unchanged — assert the old reference is intact).
- A read holding an index reference across a swap observes a single consistent index, never a partially-updated one.
- Deleting a file mid-scan is a logged skip, not a crash.
- Every public function has at least one unit test; each test generates its own fixture tree (no shared dataset).
- `tsc --noEmit` passes; no `any`/`as`/`!`/`@ts-ignore`; `gate-vendor-boundary.sh` confirms `node:fs` and the watcher appear only under `vendors/`.

**Verification method** — the commands/tests/probes that prove them:

- `./scripts/test-insightbrowser.sh` — unit tests green, coverage ≥ 90 per `plgg-test.config.json`.
- Watcher tests drive an injected fake watcher and clock (no sleeps, no real fs events), asserting the new-value/old-value-intact property directly.
- One test generates a real fixture tree on disk, runs the real walk, and asserts the record set — the scanner is proven against real fs, per `test`.
- `./scripts/check-all.sh` green.

**Gate** — what must pass before approval:

- `./scripts/check-all.sh` green, coverage ≥ 90.
- The skip-and-collect behavior demonstrated on a real malformed fence: the scan completes, the error is reported, the process lives.
- The index-swap consistency property covered by a test, not just asserted in prose.

## Considerations

- **The `YamlMap` grammar bounds the tag-group model.** `YValue` permits scalars, a sequence of scalars, and a **one-level** map of scalars — nothing deeper. The mission's tag groups (a group declaring its variations) must fit inside that subset, or plgg-md must be extended upstream and released. Check this against the real front matter in `.workaholic/` **before** modeling, and note it early if it does not fit (`/home/ec2-user/projects/plgg/packages/plgg-md/src/Yaml/model/YamlValue.ts`).
- **Duplicate keys are an error, not last-wins**, and dates must be quoted strings. Real `.workaholic/` front matter (e.g. `created_at: 2026-07-15T00:42:35+09:00`) is unquoted — verify plgg-md accepts the repo's existing corpus before assuming it does; a whole-corpus parse is the cheapest check.
- **Hot reload has no upstream precedent to copy**, so the watcher's failure modes are this ticket's to discover: editor atomic-rename saves (many editors write a temp file and rename, which some watch APIs report as delete+create), rapid successive saves, and a file moved between roots (`packages/insightbrowser/src/vendors/`).
- **The swap semantics are the subtle part.** "Immutable index, new value per reload" is easy to state and easy to violate under a reference held across an `await` in an entry point. Nail it here — tickets 3+ (SSR/REST/MCP) all read through it.
- **`discoverPaths` collapses `foo.md` and `foo/index.md`** onto one route; InsightBrowser must not, since both are distinct documents in a knowledge base (`/home/ec2-user/projects/plgg/packages/plgg-server/src/Ssg/usecase/writeStatic.ts`).
- **This repo's own `.workaholic/` is the first real corpus** — the index is testable against the very tree it lives in, which is the cheapest realistic fixture available.

---

**Depends on:** `20260715004234-repository-skeleton-and-dependency-contract.md`. **Next:** `20260715004236-ssr-browsing-and-heading-auto-numbering.md`.

---

## Progress — 2026-07-15 (night drive)

**Status: partially implemented, then BLOCKED on a dated external dependency.** The unblocked work is committed and green; the ticket stays in `todo` because its front-matter half cannot be done yet.

### Blocked: the front-matter projection

**The blocker, established empirically rather than assumed:** the published **`plgg-md@0.0.1` does not parse front matter at all.** Its model is `Frontmatter = { layout: Option<SoftStr> }` — `parseFrontmatter` detects a flat `layout:` marker, **discards the rest of the block**, and exposes no `data`, no `YamlMap`, no `YValue`. (Its own doc comment says so: *"No nested-YAML parsing"*, per plggpress's spike decision §6b.) The `YamlMap` model this ticket's Key Files describe is `plgg-md` **0.0.2** — read from the monorepo *source*, which is ahead of the registry.

`plgg-md@0.0.2` was published 2026-07-09 and, under this environment's `min-release-age=7` supply-chain control, becomes consumable **2026-07-16 09:11** (~32h after this drive). See `docs/adr/0005-pinned-toolchain-under-min-release-age.md`.

Why it was not worked around:
- **Overriding `min-release-age`** — declined. It is a security control; disabling it to make a build go green inverts its purpose.
- **A sibling checkout of plgg-md** — forbidden outright by the mission and `docs/adr/0001-npm-only-plgg-family-contract.md` ("never consumed from a sibling checkout").
- **Hand-rolling a YAML parser** — declined. This ticket says "no hand-rolled YAML" for good reason, and a private parser would fork the corpus's grammar from the one plggpress and plgg-cms use.

So `Document` carries `path` + `source` today and **deliberately omits `frontMatter`** rather than faking it as `Record<string, string>` — a stand-in shape would be a lie the compiler would then help us spread.

### Implemented and green (commit below)

The parts needing no front-matter parsing — including the ticket's hardest, precedent-free requirements:

- **The multi-root scan** (`domain/usecase/scan.ts` + `domain/model/Scan.ts`): walks `.workaholic/`, `docs/`, `packages/`; prunes `node_modules`/`dist`/`.git`/`outputs`/`coverage`; markdown only, never a dotfile. Roots are a **parameter**, not a constant, so a plgg/qfs docs site scans its own tree.
- **Skip-and-collect**: an unreadable file, an un-stattable directory, and a directory that stats-but-cannot-be-listed are each skipped and reported into the index's `errors`. One bad file cannot take the index — or the server — down.
- **The immutable index** (`domain/model/Index.ts`): keyed by document path, every mutator returns a **new** value.
- **Reload + the swap semantics** (`domain/usecase/reload.ts`) — the requirement this ticket calls its subtlest, and the one with **zero precedent anywhere in the plgg family**. Only the changed document is re-read (O(1), not O(n)); a burst is debounced into **one** swap; last-change-per-path wins; a changed-but-unreadable file is treated as removed (the editor atomic-rename case). `IndexRef` is the single mutable cell holding one immutable value, so a reader that calls `current()` once **cannot** observe a torn index — safe by construction, no locking.
- **The `node:fs` adapter** (`vendors/nodeFileSystem.ts`) behind the domain's `FileSystem` seam, so the gate proves `node:fs` appears nowhere under `domain/`.

**Verification:** `./scripts/check-all.sh` exits **0** — 40 tests, coverage 100% statements / 94.59% branches / 100% functions / 100% lines. The scan is proven against **both** the fake seam and a **real temp-dir tree** (`vendors/nodeFileSystem.spec.ts`), because a fake that agrees with a wrong model proves nothing.

### Bugs this work surfaced

1. **`isDocumentFile` and `asDocumentPath` disagreed on extension case.** The walk accepted `README.MD` case-insensitively; the caster demanded lowercase `.md`. A `.MD` file was therefore walked and then silently rejected into the scan's collected errors instead of being indexed. Both are now case-insensitive, and `Vocabulary.spec.ts` pins the agreement.
2. **Speculative API removed.** `emptyIndex` and `withErrors` were exported, never called, and never tested. Deleted rather than propped up with tests (`workaholic:design` / `sacrificial-architecture`).
3. **An unreachable branch** in `listDocuments`' comparator (`a === b` cannot occur across unique Map keys) — simplified away rather than left as permanently-uncovered code.

### What remains (unblocks 2026-07-16 09:11)

1. Bump `plgg-md` to `^0.0.2`; add it to `dependencies` (the gate permits it — plgg-family).
2. Add `frontMatter` to `Document`; project `Option<YamlMap>` inward through `asXxx(unknown) => Result<...>` casters, matching `YValue`'s five patterns.
3. Add the tag-dimension reverse maps to `Index`, and the `ListQuery`/`ListResult`/`getDocument` query surface named after `plgg-cms/src/content/Query`.
4. **Verify the `YamlMap` grammar against this repo's real front matter before modelling** — `YValue` allows scalars, a sequence of scalars, and a **one-level** map only; duplicate keys are an error, not last-wins; dates must be quoted. This repo's own tickets carry unquoted `created_at: 2026-07-15T00:42:35+09:00`. A whole-corpus parse is the cheapest check, and if it fails, plgg-md must be extended upstream.
5. Structured JSON logs for scan start/complete/error (`docs/adr/0006`).
