---
created_at: 2026-08-17T13:04:10+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff:
---

# Seventy-seven code spans across the docs site never form

## Overview

Minted while fixing ticket `20260817110309-the-docs-site-production-build-fails-on-blueprint-md.md`,
which established the root cause: **VitePress 1.6.4's renderer does not form an inline code span whose
opening and closing backticks sit on different source lines.** The backticks survive into the rendered
HTML as literal characters and the span's contents render as prose.

That ticket fixed only the three spans whose contents were angle-bracketed (`<path>`, `<name>`), because
those reached the Vue compiler as unclosed elements and broke `npm run docs:build`. Its own
Considerations forbade a blind reflow of the rest, so the remainder was measured and left:

**154 literal backticks survive into the rendered output, outside all `<code>`/`<pre>` regions, across
18 of the 38 pages** — i.e. roughly **77 unformed code spans**. Measured after the fix, by rendering
each page through `createMarkdownRenderer` and counting backticks in the HTML outside code regions:

```
blueprint.md 14   guide/cli.md 40        drivers.md 30      guide/concepts.md 10
guide/shell.md 10 cookbook/faq.md 6      documentation-map.md 6  server.md 6
cookbook/automation.md 4  guide/passphrase.md 4  language.md 4  guide/repository.md 8
cookbook/gdrive.md 2  cookbook/github.md 2  guide/account-model.md 2
guide/design-snapshot.md 2  roadmap.md 2  security/threat-model.md 2
```

Nothing is broken in the build sense — these pages compile and serve. What they do is show readers a
literal `` `qfs account ``…`` add` `` instead of code, and lose the styling on whatever the span
contained. The new `docs-build` CI job does **not** catch them: unformed spans only fail the build when
they happen to contain something the Vue compiler reads as a tag.

**Three of the four worst pages are generated.** `drivers.md` (30), `language.md` (4) and `server.md`
(6) are rendered from the binary by `cargo run -p xtask -- gen-docs` and must never be hand-edited, so
their share is a fix in the **renderer's line-wrapping**, not in the page. `guide/cli.md` (40) and
`blueprint.md` (14) are hand-written. That split is why this is its own ticket and not a tidy-up.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:implementation` / `policies/objective-documentation.md` — the docs are a deliverable, and
  a page that renders its own markup at the reader is not one
- `workaholic:operation` / `policies/ci-cd.md` — a check that is not automated is not a check

## Key Files

- `docs/guide/cli.md` (40 backticks) and `docs/blueprint.md` (14) — hand-written; a reflow fixes these.
- `packages/qfs/crates/qfs/src/docs.rs` and the `gen-docs` renderer path — the source of `drivers.md`,
  `language.md` and `server.md`. Whatever wraps lines there is what emits the straddling spans; the
  `docs_drift_golden` unit test in the same file is the pattern that would defend a fix.
- `.github/workflows/ci.yml` — the `docs-build` job added by ticket 20260817110309. It proves pages
  compile; extending it (or a unit test) to also assert zero stray backticks is this ticket's
  enforcement question.
- `docs/.vitepress/config.mts` — the markdown config is minimal (only the custom `qfs` language), so
  nothing here is causing this; a markdown-it option that permits multi-line spans would be a
  one-line alternative fix if one exists.

## Implementation Steps

1. Re-measure to a current baseline: render every `docs/**/*.md` through VitePress's own
   `createMarkdownRenderer`, strip `<pre>`/`<code>` regions, count residual backticks per page.
   (The probe used to produce the numbers above is not committed — step 4 decides whether it should
   be.)
2. Establish whether a renderer option fixes the whole class at once before editing any prose. CommonMark
   *does* permit a code span to contain a line ending, so this may be a markdown-it configuration or
   version issue rather than an authoring rule. If one option fixes all 18 pages, prefer it — it also
   fixes the generated pages without touching the generator.
3. If no such option exists: reflow the two hand-written pages so no span straddles a newline, and fix
   the `gen-docs` renderer's wrapping so the three generated pages stop emitting them. Regenerate and
   confirm `gen-docs --check` is clean.
4. Decide and record the enforcement: a committed scan script wired into the `docs-build` job, or a
   `docs.rs` unit test for the generated half, or both. A count that nothing re-measures will drift
   straight back.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Zero literal backticks survive into rendered output outside `<code>`/`<pre>` across all pages under
  `docs/`.
- `npm run docs:build` still exits 0 and `cargo run -p xtask -- gen-docs --check` still exits 0 (the
  generated pages are regenerated, never hand-edited).
- No page's wording changes — only line breaks inside code spans, or generator wrapping. A
  `git diff --word-diff` over the hand-written pages shows no changed words.
- The check is automated, so the count cannot silently return.

**Verification method** — the commands/tests/probes that prove them:

- The step-1 render-and-count scan, re-run after the change, printing zero for every page.
- `npm run docs:build; echo $?` and `cargo run -p xtask -- gen-docs --check; echo $?`, both 0.
- `git diff --word-diff docs/guide/cli.md docs/blueprint.md` — no changed words.
- The new automated check, shown red on a deliberately straddled span and green after reverting it.

**Gate** — what must pass before approval:

- `npm run docs:build` exits 0.
- `cd packages/qfs && cargo test --workspace` and `cargo run -p xtask -- gen-docs --check` exit 0.

## Considerations

- **Try the renderer option before the reflow.** Roughly half the affected spans are on generated
  pages, where a reflow is not even available as a fix — so a configuration answer is worth more than
  it looks, and CommonMark being on its side makes one plausible. Reflowing 77 spans by hand and
  *then* discovering a one-line option would be the bad outcome.
- The build-breaking subset is already fixed and CI-defended; this is a legibility defect, not an
  outage. It should not be batched with anything urgent.
- `docs/guide/cli.md` is separately noted in `docs/documentation-map.md` as written before `agent` and
  `view` shipped. If that page is being rewritten anyway, its 40 backticks come along for free — worth
  checking before reflowing it in isolation.

## Final Report

**This ticket's premise was false, and establishing that was the work.** No prose was reflowed, no
generator was changed, and none should be. Step 2 — "establish whether a renderer option fixes the whole
class at once **before editing any prose**" — is exactly what caught it, one ticket after the
mis-measurement that created this ticket.

### The measurement that made this ticket was invalid

The figure "154 stray backticks across 18 of 38 pages (~77 unformed spans)" came from rendering each page,
stripping `<pre>` and `<code>` regions, and counting the backticks left over. That measure does not mean
what it was taken to mean. Inspecting the actual match contexts:

- **VitePress writes each heading's RAW MARKDOWN into its permalink `aria-label`.** A heading
  `### \`qfs run\` — execute one statement` renders an anchor carrying
  `aria-label="Permalink to &quot;\`qfs run\` — execute one statement&quot;"`. The backticks are correct,
  are inside an attribute, and are never displayed. This accounts for most of the count, and for **all
  40** on `docs/guide/cli.md` and **all 14** on `docs/blueprint.md`.
- **HTML comments are preserved verbatim.** `docs/drivers.md`'s 30 are its two generator banners —
  `<!-- GENERATED by \`cargo xtask gen-docs\` … -->` — which is why the three "worst" pages were the
  generated ones: they carry generator banners, not broken spans.

So the pages named were never broken, and the split between "hand-written" and "generated" that made this
ticket look structural was an artifact of which pages have the most headings and comments.

### The real mechanism, corrected

Ticket `20260817110309` recorded the cause as "VitePress does not form an inline code span whose backticks
straddle a newline." **That is too broad.** Multi-line code spans work fine. Tested through VitePress's own
renderer:

| Case | Code span forms? |
| --- | --- |
| `` `CREATE TABLE FOO ⏎ BAR` `` — plain continuation | **yes** (`<code>CREATE TABLE FOO BAR</code>`) |
| the same inside a bullet's continuation line | **yes** |
| `` `CREATE TABLE OF ⏎ x <name>` `` — tag *mid*-line | **yes** (`&lt;name&gt;`, correctly escaped) |
| `` `CREATE TABLE <path> OF ⏎ <name>` `` — continuation **starts** with `<tag>` | **no** |

The failing case is a **block-level** event, not an inline one. With `html: true` (VitePress's default), a
line whose first non-whitespace is an angle-bracketed token is taken as an **HTML block**, which ends the
paragraph before the closing backtick is ever reached — the render is
`<p>a \`CREATE TABLE <path> OF</p>` followed by a raw `<name>`. The inline `backticks` rule is present and
enabled and never gets the chance to run across the two lines. That is why the payload is always an
angle-bracketed token: the token is not a *consequence* of the broken span, it is the *cause*.

**So there is no renderer option to reach for** (step 2's preferred outcome): the behaviour follows from
`html: true`, which the site needs, and it is CommonMark-conformant rather than a defect. The authoring
rule is the fix, and it is narrow — never begin a continuation line with `<…>` — not "never wrap a code
span".

### The class is already closed, and already defended

- **Source-side scan** for the real pattern (a continuation line starting with `<tag>` while a backtick run
  is open, fences and frontmatter masked) across all 38 pages: **0 candidates**.
- **Authoritative scan** — every page rendered and then parsed with `@vue/compiler-dom`, the same compiler
  `vitepress build` uses: **0 of 38 pages rejected**.
- **Step 4's enforcement question needs no new check.** Every instance of this class emits a raw element
  that fails the Vue compiler *by construction* — that is the class's definition — so the `docs-build` CI
  job added by `20260817110309` catches all of it. A separate backtick-counting check would have been
  strictly worse than nothing here: it is the very measure that produced this false ticket.

### Changes

- No source, prose, or generator change. Two records corrected so the false figure does not propagate:
  - `.workaholic/stories/work-20260817-124626.md` — the "Seventy-seven code spans" concern is marked
    **WITHDRAWN** with the reason, rather than left to be extracted into the feedback stream at ship time
    as a real concern.
  - This Final Report carries the corrected mechanism, since this ticket is where a future reader
    investigating this class will land.

### Verification

- `node` probe through `createMarkdownRenderer('docs')` over the six-case matrix above — the four passing
  cases produce `<code>`, the two `<tag>`-first cases do not.
- `md.inline.ruler.__rules__` confirms `backticks` present and enabled; `md.options` shows
  `html: true, breaks: false`, so the split is the HTML-block rule and not a disabled inline rule.
- Match-context inspection on `docs/guide/cli.md`, `docs/drivers.md` and `docs/blueprint.md` — every
  counted backtick is inside an `aria-label` attribute or an HTML comment.
- Source-side scan: 0 candidates. `@vue/compiler-dom` over all 38 rendered pages: 0 rejected.
- `npm run docs:build` exits 0.

### Discovered Insights

- **Insight**: A metric invented to measure a defect can manufacture one. "Backticks surviving outside
  `<code>`" sounded like a direct proxy for "unformed code span" and was not: it counts rendered
  *attributes* and *comments*, both of which legitimately carry markdown. The ticket it produced named 18
  pages, ranked them, and inferred a structural hand-written/generated split — all from noise.
  **Context**: The guard that worked was already in the process: parse with the real downstream consumer
  (`@vue/compiler-dom`) instead of pattern-matching its input. That scan said 0 both before and after, and
  disagreeing with the invented metric is what should have been believed immediately.
- **Insight**: `html: true` makes any line starting with `<word>` a paragraph-terminating HTML block, so a
  markdown wrapping rule can break a code span two lines later. The defect surfaces as an inline-formatting
  failure and is a block-parsing event, which is why the first root-cause statement generalised wrongly.
  **Context**: The practical authoring rule for `docs/` is only "do not begin a continuation line with
  `<…>`" — much cheaper than "keep every code span on one line", and it is what a future contributor
  should be told.
