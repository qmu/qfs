---
created_at: 2026-08-17T11:03:09+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff:
---

# The docs site production build fails on blueprint.md

## Overview

Found while verifying the architecture page's gate (ticket
`20260817102723-document-the-architecture-as-built-the-crate-map-the-engine-layering-the-state-stores-and-the-faces.md`).

`npm run docs:build` (`vitepress build docs`) fails at `docs/blueprint.md`:

```
SyntaxError: [plugin vite:vue] docs/blueprint.md (271:130): Element is missing end tag.
```

The position is in the **generated** SFC, not the source line. Rendering `blueprint.md` through
VitePress's own `createMarkdownRenderer` and reading line 268 of the HTML localizes it: the
inline-code span opened at `` `CREATE TABLE <path> OF `` on source line 266 and closed by
`` <name>` `` on line 267 is **not** parsed as code — the rendered output carries literal
backticks and raw `<path>` / `<name>` elements, which the Vue compiler then reads as unclosed
tags. Everything from that opening backtick to the end of the list item loses its code spans
(`` `TYPE` ``/`` `OF` `` on the same line render literally too).

Nothing raw is wrong in the source when read by eye: a scan that tracks fenced blocks and
backtick runs across newlines finds **zero** raw HTML tags outside code anywhere in
`blueprint.md`. So the defect is in how that one multi-line span interacts with the renderer, and
the fix is a rendering fix, not an edit to what the section says.

**Why nobody noticed.** The project runs the site as `docker compose up docs`, i.e.
`vitepress dev`, which compiles pages **on demand** — a page you do not open never compiles. Only
the production build walks every page, and no gate runs it: `.github/workflows/ci.yml` has no
docs job, and `CLAUDE.md` lists no docs-build command. The break is therefore invisible to every
check the project currently has.

`docs/guide/architecture.md` and `docs/documentation-map.md` both compile and serve correctly
(verified in Chromium against `vitepress dev` on port 4101), so this is not a new-page problem.
The build aborts at the first failing page, so `blueprint.md` may not be the only one — step 2
below is to find out rather than assume.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` / `policies/ci-cd.md` — a check that is not automated is not a check
- `workaholic:implementation` / `policies/objective-documentation.md` — the docs are a deliverable

## Key Files

- `docs/blueprint.md` lines 265-267 — the multi-line inline-code span that does not survive the
  renderer. **The section's content is out of scope**; only its markup changes.
- `docs/.vitepress/config.mts` — `ignoreDeadLinks: true` lives here; whether the docs build should
  also fail on dead links is a decision this ticket can surface but need not take.
- `package.json` — `docs:build` is already defined and is the command a gate would run.
- `.github/workflows/ci.yml` — no docs job exists; this is where one belongs.
- `Dockerfile.docs`, `docker-compose.yml` — the dev path that hides the break.

## Implementation Steps

1. Reproduce: `npm install && npm run docs:build` from the repository root, and confirm the same
   `blueprint.md` failure.
2. Find every occurrence, not just the first: render each `docs/**/*.md` through
   `createMarkdownRenderer` and scan the resulting HTML for raw `<tag>` output, so the whole class
   is fixed in one pass rather than one page per build attempt.
3. Fix the markup, not the prose — keep each inline-code span on one line (or otherwise make the
   renderer see it) so `<path>`/`<name>` are escaped as code. Do not reword what the section says.
4. Re-run `npm run docs:build` to a clean exit.
5. Add a docs-build job to `ci.yml` so the production build is walked on every push; decide and
   record whether it also turns off `ignoreDeadLinks`.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `npm run docs:build` exits 0 from the repository root.
- No page under `docs/` renders a raw HTML element that the source intended as inline code
  (the step-2 scan reports zero).
- `docs/blueprint.md`'s §5 text is unchanged apart from whitespace/line breaks inside the code
  spans — a diff shows no changed words.
- CI fails if the docs build breaks again.

**Verification method** — the commands/tests/probes that prove them:

- `npm run docs:build`, exit code recorded.
- The step-2 scan, re-run after the fix, printing zero findings.
- `git diff --word-diff docs/blueprint.md` shows only markup movement.
- The new CI job is red on a deliberately broken page in a scratch branch, then green after
  reverting it.

**Gate** — what must pass before approval:

- `npm run docs:build` exits 0.
- `cd packages/qfs && cargo run -p xtask -- gen-docs --check` exits 0 (no generated page touched).

## Considerations

- The renderer's behaviour here is the actual unknown; the fix should not be a blind reflow of
  every long line in `blueprint.md`. Localize with the render-and-scan probe first (`docs/`, via
  VitePress's own renderer), then change only what the probe names.
- `ignoreDeadLinks: true` means the build says nothing about broken links today. Turning it off
  in the same change would mix a real fix with a possibly large link cleanup — surface the count,
  let the developer decide.

## Final Report

Development completed as planned.

**Root cause, established empirically rather than assumed.** The ticket suspected the multi-line inline
code span at `blueprint.md` L266-267 and was right about the location; the mechanism is that
**VitePress 1.6.4's renderer does not form an inline code span whose opening and closing backtick runs
sit on different source lines.** Reduced to a two-line minimum through VitePress's own
`createMarkdownRenderer`:

```
input:  "- a `CREATE TABLE <path> OF\n  <name>` b"
output: "<li>a `CREATE TABLE <path> OF<name>` b</li>"     ← backticks literal, newline swallowed
input:  "- a `CREATE TABLE <path> OF <name>` b"           ← same span, one line
output: "<li>a <code>CREATE TABLE &lt;path&gt; OF &lt;name&gt;</code> b</li>"
```

So the `<path>`/`<name>` reach the Vue compiler as real, unclosed elements and it raises "Element is
missing end tag". Note this is *not* CommonMark behaviour — the spec permits a code span to contain a
line ending — but markdown-it is bundled inside the `vitepress` dist here, so the two could not be
isolated from each other and no blame is assigned beyond "this renderer, at this version". The fix
taken is therefore the configuration-independent one the ticket asked for: keep each span on one line.

**Step 2 found the whole class, and it was larger than one span.** The first scan was written against a
tag allow-list and under-reported, because `path` is a legitimate SVG element name and got filtered out
as "known". It was replaced with the honest probe: render each page with VitePress's renderer, then parse
the result with `@vue/compiler-dom` — **the same compiler the production build uses**, so the criterion
is the build's own, and unlike the build it does not stop at the first failing page. That found
**three** unclosed-element errors, all on `blueprint.md`, from three separate straddling spans:

| Source | The span |
| --- | --- |
| L266-267 | `` `CREATE TABLE <path> OF` `` / `` `<name>` `` |
| L351-352 | `` `CREATE TRANSFORM` `` / `` `<name>` `` |
| L368-369 | `` `INPUT OF` `` / `` `<name>` `` |

Only the first broke the build, because the build aborts at the first page — the other two would have
surfaced as two more failed builds one at a time. All three are fixed in this one pass, which is what
step 2 exists for.

### Changes

- `docs/blueprint.md` — three spans reflowed onto single lines. **No words changed**:
  `git diff --word-diff=porcelain` reports zero added/removed words, only moved line breaks.
- `.github/workflows/ci.yml` — new `docs-build` job (`checkout` → `setup-node 24.x` → `npm install` →
  `npm run docs:build`), with its own `working-directory: .` because the workflow's default is
  `packages/qfs`. Its comment records why the break was invisible: `docker compose up docs` runs
  `vitepress dev`, which compiles pages on demand, so a page nobody opens never compiles.

### Verification

- `npm run docs:build` — **exit 1 before** (`docs/blueprint.md (271:130): Element is missing end tag`),
  **exit 0 after** ("build complete in 13.72s").
- The Vue-parse scan across all 38 pages: **3 errors on 1 page before, `0 page(s) with errors` after.**
- `git diff --word-diff=porcelain docs/blueprint.md` — no changed words, whitespace only.
- The CI job's exact command proven red **and** green locally: a scratch `docs/zz-probe.md` carrying one
  deliberately straddled span → `npm run docs:build` exit **1**; removing it → exit **0**. The job body
  is that single command, so this discharges the criterion's substance without burning a scratch-branch
  CI run. YAML parsed and the job's step list confirmed.
- `cargo run -p xtask -- gen-docs --check` — exit 0, so no generated page was touched.

### Deferred decision: `ignoreDeadLinks`

The ticket asked for the count, not the cleanup. Measured by flipping `ignoreDeadLinks` to `false` and
rebuilding: **8 dead links, all in `blueprint.md`, all pointing at Rust source files** (e.g.
`./../packages/qfs/crates/core/src/ddl/server.rs`) — VitePress resolves links only against pages, so a
link to a `.rs` file can never resolve. Turning the flag off therefore needs a decision about *how*
source references should be written (GitHub URLs, an ignore pattern, or plain code spans), which is the
developer's call and would mix a policy change into a rendering fix. Left `true`, and the new job's
comment states in writing that it proves pages **compile**, not that links resolve. The config was
restored byte-identically (`git status --porcelain` clean on it).

### Ticket minted

`20260817130410-seventy-seven-code-spans-across-the-docs-site-never-form.md` — the same root cause has
**154 literal backticks surviving into rendered output across 18 of the 38 pages** (~77 unformed
spans). They do not break the build, because only an angle-bracketed payload does that, so they are
outside this ticket's acceptance and the ticket's own Considerations forbade a blind reflow. Measured
rather than estimated. Notably three of the four worst pages (`drivers.md` 30, `server.md` 6,
`language.md` 4) are **generated** by `gen-docs`, so their share is a renderer fix, not a page edit.

### Discovered Insights

- **Insight**: A scan built on an allow-list of "known good" HTML tags will silently miss placeholders
  that collide with real element names — `<path>`, `<text>`, `<line>`, `<use>`, `<g>`, `<source>` are
  all SVG elements and all plausible documentation placeholders. Parsing the rendered HTML with the
  project's actual downstream compiler has no such blind spot and needs no list to maintain.
  **Context**: The first version of this scan reported `<name>` only and would have led to fixing one
  span, leaving two more builds to fail one at a time — exactly the "one page per build attempt" loop
  step 2 was written to avoid.
- **Insight**: A dev server that compiles on demand is not a check. `vitepress dev` never compiled
  `blueprint.md` unless somebody navigated to it, so a page could sit unbuildable indefinitely while
  the documented workflow (`docker compose up docs`) looked healthy. Any on-demand toolchain needs a
  separate whole-corpus build in CI to mean anything.
  **Context**: This is the second CI-coverage gap found in this mission — the sibling ticket
  20260817111530 records that `viewer-check-all` installs Node only, so the runtime matrix it proves is
  narrower than a developer's. Both are the same shape: the local path and the CI path disagree about
  what is covered, and nothing said so.
