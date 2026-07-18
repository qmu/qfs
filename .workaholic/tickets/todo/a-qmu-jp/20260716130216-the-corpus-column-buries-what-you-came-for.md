---
created_at: 2026-07-16T13:02:16+09:00
author: a@qmu.jp
type: enhancement
layer: [UX]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# The corpus column buries what you came for

## Why it looks like this — three causes, and none of them is "plggmatic is off-limits"

The developer opened the demo and asked why it does not look like the plggmatic
reference. Looked at rather than guessed, with a screenshot of
`/?type=enhancement`:

### 1. The document list is the LAST thing on the page

Measured order inside `.column-corpus`:

```
count → facet ×7 → errors → documents
```

**The corpus — the thing the page exists to show — renders below the error
section**, under seven facet groups. At 900px you cannot see a single document
without scrolling past everything else. The page says `1–8 of 8 document(s)`
at the top and then shows you eight facets and an error.

This is not a styling problem and no token vocabulary fixes it. It is the
order the elements are appended in `corpusColumn`.

### 2. Facets that cannot filter anything take the space

From the same screenshot: `a@qmu.jp (8)` and
`build-insightbrowser-on-the-plgg-family (8)`, next to a list of **8**
documents. A facet value covering every matched document is a link that
changes nothing — it is a control that does nothing, occupying the room the
documents needed.

`depends_on` is worse: its values are whole filenames
(`20260715004235-markdown-scanner-and-frontmatter-index.md (2)`), so each chip
wraps to two lines. A dimension whose values are unique-ish identifiers is not
a dimension, it is a list of primary keys.

Note the config already has `hide` for this, which is the escape hatch and not
the answer: every repository would have to discover the noise and hide it by
hand. The default should be honest.

### 3. Every colour is a literal, because nobody ever specified the look

`entrypoints/columns.ts` carries ~3.1KB of hand-written CSS with `#ddd`,
`#fafafa`, `#666`, `#a33` inline. No tokens, no theme, no dark mode.

**That is exactly what the acceptance criterion asked for**, and that is the
uncomfortable part:

> Column-accretion UI on plgg-view: page links resolve sideways, traversal
> legible on screen and **in the URL**

A claim about BEHAVIOUR. It is met, and it is checked `[x]`. Nothing in the
mission ever said the thing should be pleasant to look at, so the sheet grew to
exactly the size that makes columns be columns and stopped.

## plggmatic is NOT the blocker — checked, not assumed

| | |
| --- | --- |
| plggmatic's `plgg-view` | `^0.0.2` |
| our `plgg-view` | `^0.0.2` |
| npm latest | `0.0.2` |

**The same version.** There is no capability we lack. plgg-view exports the
MECHANISM — `css$`, `style`, `escapeCss`, `renderCssRule`, `collectCssRules`,
`collectCss` — and deliberately **no design language**: no colour atoms, no
tokens, no theme (checked: 98 exports, none of them a colour).

plggmatic built its own on top, and its doctrine is this house's:

- `Style/model/token.ts` — a **closed union**, so `bg("blurple")` or a bare
  `bg("primary")` without a variant is a **compile error**. "The type-driven win
  over stringly CSS classes."
- A matrix: 5 semantic roles × 4 variants + a 5-member neutral scale = 25
  tokens, so adding a role is one union edit whose fallout `tsc` drives.
- `Palette`, `asPalette`, `colorHex`, `paletteHex`, `Theme`, light/dark.
- **The earned-place rule**: the token file is a *seed*, not a catalog — each
  entry earns its place from a concrete consumer.

ADR 0002 forbids **depending on plggmatic** and **porting its code**. It does
not forbid learning from it — CLAUDE.md says to follow it *in spirit*. So this
is buildable today and simply has never been built.

## Implementation Steps

Staged so the cheap fix is not held hostage by the design one.

1. **Reorder the corpus column** — documents directly after the count; errors
   last. One move, and it is the single biggest thing wrong with the page.
2. **Drop facet values that cannot filter.** A dimension whose every value
   covers the whole matched set filters nothing; a dimension whose values are
   unique per document is a primary key. Both are noise BY DEFAULT, with `hide`
   remaining the per-repository override rather than the mechanism.
3. **Build the Style seed** on plgg-view, following plggmatic in spirit and not
   by copy: a closed union of roles, `colorHex`, and the literals replaced by
   `--ib-*` custom properties. **Earn each token from a consumer that exists**
   — this page needs surface, text, muted, border, danger and roughly nothing
   else. Do NOT pre-build a 25-token matrix for a page with six colours in it.
4. **Dark mode only if it is asked for.** `prefers-color-scheme` is two lines
   once the tokens exist, and zero without them — which is the argument for
   tokens, not an argument for shipping a theme nobody requested.

## Policies

- `workaholic:design` / `policies/interaction-design-standard.md` — the `UX`
  layer, and the reason this is a defect and not a preference: the page
  promises `1–8 of 8 document(s)` and then puts the eight below an error
  section. A control that does nothing (`a@qmu.jp (8)`) is the same failure as
  a count that lies — the screen says something that is not so.
- `workaholic:design` / `policies/no-dark-patterns.md` — a facet that cannot
  change the result set is noise the reader has to learn to ignore. Do not make
  people learn which of your controls are real.
- `workaholic:implementation` / `policies/coding-standards.md` — universal, and
  load-bearing for step 3: the token vocabulary is a CLOSED UNION, so a typo is
  a compile error. Stringly CSS classes are the thing being replaced. No `as`
  to force a colour through.
- `workaholic:implementation` / `policies/directory-structure.md` — universal.
  A `Style/` module lives under the package's own tree; plggmatic is not a
  dependency and must not appear in any `package.json` (ADR 0002).
- `workaholic:implementation` / `policies/objective-documentation.md` — if the
  token vocabulary is fixed as a matrix rather than earned per consumer, that
  is a decision with a real alternative and needs recording. plggmatic recorded
  exactly this (its D9 amendment).
- `workaholic:design` / `policies/sacrificial-architecture.md` — the sheet is
  3.1KB and inline. Do not pre-optimise it into a framework; grow the seed as
  components demand tokens, which is plggmatic's own stated doctrine.
- `workaholic:planning` / `policies/accessibility-first.md` — tokens make
  contrast checkable rather than incidental. `#666` on `#fafafa` was chosen by
  nobody.

## Key Files

- `src/entrypoints/columns.ts` — `corpusColumn` (the append order, step 1) and
  `STYLE` (~3.1KB literal sheet, step 3).
- `src/domain/usecase/tagGroups.ts` — `tagGroupsOf`, where a value that covers
  everything is still emitted (step 2). It already takes the FILTERED set, so
  it has what it needs to know.
- `src/domain/model/Config.ts` — `hide`, the existing per-repo override that
  must stay an override.
- Reference only, never imported: `/home/ec2-user/projects/plggmatic/packages/
  plggmatic/src/Style/` — `model/{token,palette,theme,scheme,metric,
  breakpoint,appearance}.ts`.

## Quality Gate

### Acceptance Criteria

- The first documents are visible without scrolling at 1400×900 on `/`.
- Errors render after the documents, not before.
- No facet value is offered whose count equals the matched total for a
  single-valued dimension, and `depends_on`-shaped dimensions do not render as
  chips of filenames — by default, with no `hide` configured.
- Every colour in the served sheet comes from a token; no bare hex outside the
  palette module.
- The token type is a closed union: an unknown role fails `tsc`.
- `/` with JS off is unchanged in behaviour (this ticket adds no client JS).

### Verification Method

```sh
# the screenshot IS the gate for step 1 — the bug was invisible to 269 tests
~/.cache/ms-playwright/chromium-1228/chrome-linux/chrome --headless \
  --disable-gpu --no-sandbox --window-size=1400,900 \
  --screenshot=/tmp/ui.png http://localhost:4100/

# order, mechanically
curl -sf localhost:4100/ | grep -o 'class="\(documents\|errors\)"'   # documents FIRST

# no dead controls
curl -sf 'localhost:4100/?type=enhancement' | grep -oE '>[^<]+\([0-9]+\)<'

# no stray literals
grep -nE '#[0-9a-fA-F]{3,6}' src/entrypoints/columns.ts   # expect none after step 3
./scripts/check-all.sh
```

### Gate

- `./scripts/check-all.sh` exits 0.
- **A screenshot is required, not optional.** 269 green tests did not see that
  the document list was below the fold, because every one of them asserted
  `htmlOf(r).includes(...)` — presence, never position. A person opening the
  page found it in seconds. That is the third time on this branch that a person
  looking at the screen beat the suite.

## Considerations

- **Do not add plggmatic to any `package.json`, and do not port its files**
  (ADR 0002, CLAUDE.md). Read it, learn the doctrine, write our own. The
  temptation will be strongest at `token.ts`, which is the file most worth
  understanding and least worth copying.
- **Do not add a CSS toolchain.** ADR 0001 is a dependency contract and
  `scripts/gate-dependencies.sh` fails the build on a foreign runtime dep. The
  sheet stays inline; that is a feature of a tool that runs at a repository
  root with no build step.
- **The mission criterion may deserve amending.** It is checked `[x]` on
  behaviour and always will be, so nothing in the mission will ever go red for
  an unreadable page. If the look matters, the criterion has to say so — the
  alternative is that "it looks like this" is nobody's bug.
