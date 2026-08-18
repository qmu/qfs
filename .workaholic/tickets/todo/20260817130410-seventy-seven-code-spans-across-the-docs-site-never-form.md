---
created_at: 2026-08-17T13:04:10+00:00
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
