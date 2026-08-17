---
created_at: 2026-08-17T11:03:09+00:00
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
