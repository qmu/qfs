# 0002 — plggmatic is a design reference, not a dependency

**Status:** Accepted (2026-07-15), amended (2026-07-16), amended again
(2026-07-17 — plggmatic becomes the UI engine; see below)
**Ticket:** 20260715004234-repository-skeleton-and-dependency-contract.md
**Mission:** build-insightbrowser-on-the-plgg-family

## Decision

qfs-viewer's column-accretion UI follows the **idea** plggmatic proposed —
columns as projected depth, the whole navigable state living in the URL, a
sideways link rather than a screen replacement — but implements it here on
`plgg-view` directly.

**plggmatic is not a dependency**, and its code is not ported.
`scripts/gate-dependencies.sh` rejects any `package.json` naming it, checking
the exclusion *before* the plgg-family prefix rule (`plggmatic` starts with
`plgg`, so the prefix test alone would wave it through).

## Reasoning

This is the decision most likely to be quietly reversed by a future
contributor, because plggmatic looks like exactly the right dependency: it is
plgg-family, it is published, and it already implements the column model this
product wants. Recording why not is the point of this ADR.

The distinction is between a **mechanism** and a **model**:

- plggmatic's `Scene`/`Level` model is built for a *declared application* — a
  scheduler that derives menu/list/detail levels from a declaration's
  collection chain (`plggmatic/docs/specs/20260708-pragmatic-screen-transition-model.md`).
  Its flow graph is computed from `child` pointers between declared
  collections.
- qfs-viewer's columns are a *document corpus* — a markdown link resolves
  sideways into a new column. There is no declaration, no collection chain, and
  no `child` pointer; the graph is the corpus's own link structure, discovered
  by scanning.

Adopting plggmatic would mean expressing a link graph as a declared collection
chain, which is a translation, not a reuse — and translations at this layer
tend to end with the consumer bending the upstream model until it serves
neither party. What we actually want from plggmatic is the *insight* (state in
the URL, columns as a projection of depth, mode-flip is loss-free because both
renderers fold one scene), and insight transfers by reading, not by importing.

The mission states this as the constraint directly: plggmatic is "a reference
to learn from, **not a dependency**."

## Alternatives considered

- **Depend on plggmatic and use its scheduler.** Rejected: its model assumes a
  declaration with a collection chain; a document corpus has a link graph.
  Fitting one to the other distorts both. Revisit only if plggmatic grows a
  first-class corpus/link model — and then as a new ADR.
- **Port plggmatic's column renderer into this repo.** Rejected: a copy forks
  at the moment it lands, and we would own a divergent renderer with none of
  the upstream's ongoing work. The no-vendoring rule of ADR 0001 applies with
  equal force to a "temporary" copy.
- **Extract a shared column package both consume.** Rejected as premature
  (`workaholic:design` / `sacrificial-architecture`): we have exactly one
  corpus-shaped consumer and no evidence about what would actually be common.
  Worth reconsidering once qfs-viewer's column model has proven itself in
  use — abstracting before then would encode a guess.

## Consequences

- The column UI is built on `plgg-view`'s `Html`/TEA primitives.
- plggmatic's specs remain required *reading* — in particular the
  URL-holds-the-state tenet, which the SSR router honours from its first commit
  (`workaholic:design` / `modeless-design`).
- The gate makes the exclusion machine-checked. A contributor who adds
  plggmatic to a `package.json` gets a red gate naming this ADR, rather than a
  silent architectural reversal.

## Amendment (2026-07-16, HQ ticket 20260716212002)

The strategy plan renamed this repository to **qfs-viewer** and retired the
`qmu/plggmatic` repository. Its UI-engine code was ported here as a **sibling
package**, `packages/plggmatic` (git history stays in the retired repo; the
reference app went to qmu-co-jp's design-pattern work, not here).

What this amendment changes — and what it does not:

- **Changed:** "its code is not ported" no longer holds; the engine's home is
  now this repository, and the alternative "port plggmatic's column renderer"
  is moot because there is no upstream left to diverge from.
- **Unchanged:** the `qfs-viewer` package still implements its column UI on
  `plgg-view` directly and declares **no runtime dependency** on plggmatic.
  `scripts/gate-dependencies.sh` keeps rejecting any `package.json` that names
  it. Making qfs-viewer consume the in-repo plggmatic engine would be a new
  decision and a new ADR.

## Second amendment (2026-07-17, ticket 20260717020104) — plggmatic becomes the UI engine

This is the "new decision" the first amendment reserved, taken explicitly.
The strategy plan (`qmu/strategy` `docs/plan.md`) and mission `qfs-viewer-mvp`
make plggmatic — the engine ported into `packages/plggmatic` — **this
package's UI engine**: the trail's columns are to render as the engine's
column strip (static headers, internal horizontal scroll, depth never
consuming the viewport — the measured shape in
`docs/plggmatic-semantics/poc-findings.md`), and the hand-built plgg-view
column renderer retires as the sacrificial first skin it was
(`workaholic:design` / `sacrificial-architecture`).

The original Decision's reasoning has been overtaken by facts, not defeated
in argument: the mechanism-vs-model distinction assumed an upstream plggmatic
whose declared-application model this corpus would have to bend to. There is
no upstream anymore — the engine lives here, and the mission's pipeline
(one deterministic manifest generator lowering describe schemas into the
Declaration → Scene feed) is precisely the corpus-shaped front door the
original ADR said plggmatic lacked.

**What changes now:**

- `scripts/dependency-contract.mjs` / `gate-dependencies.sh` accept
  `plggmatic` as a plgg-family runtime dependency. Every other non-plgg
  dependency stays rejected, and the gate's self-test proves both directions
  each run.

**What does NOT change yet, and the wall it waits behind:**

- `packages/qfs-viewer/package.json` still declares **no** plggmatic
  dependency, and the hand-built renderer still renders. The registry's
  `plggmatic` is unusable: `0.1.0` (published 2026-07-04) predates the
  engine port, and its tarball — verified 2026-07-17 by unpacking it —
  contains **no code at all** (`package.json` and `README.md` only; the
  `dist/` its own manifest points at is absent). A `file:` sibling
  dependency breaks the npx smoke (the packed tarball cannot resolve
  `file:../plggmatic` in a consumer's tree), and the bin executes `src`,
  so consumers resolve the dependency from the registry.

**The sequence, so nobody shortcuts it:**

1. The developer publishes the ported engine from this repository, at a
   version above 0.1.0 (npm credentials are the developer's; the request is
   routed via HQ). `min-release-age` applies per ADR 0005: the smoke's
   scoped override resolves a fresh publish immediately; a floor-respecting
   consumer waits out the seven days.
2. `qfs-viewer` declares the registry `^version` — the gate now permits it.
3. The strip re-render lands and the hand-built renderer retires
   (ticket 20260717020104's remaining scope).

Opening the gate before the dependency exists is deliberate and harmless:
nothing declares plggmatic yet, and the npx smoke goes red on any premature
`file:`/unpublished declaration — which is exactly the check that keeps this
sequence honest.

### The sequence completed (2026-07-17, later the same day)

The developer published **plggmatic@0.2.0** from this repository (the
empty-tarball fault was `files`/`prepack` in the engine's manifest — fixed
on its own branch; the 0.2.0 tarball was verified to carry the full `dist`).
With the wall down, the rest of the sequence landed at once:

- `packages/qfs-viewer` declares `plggmatic: ^0.2.0` — the first and only
  package the gate's 2026-07-17 opening was for.
- The trail's columns render as the **engine's strip**: engine columns
  (`row`/`column`/panes) in one engine row, the engine's sticky `colHead`
  as every column's static header (its title is the collapse link), the
  engine's `schemeCss`/`metricCss`/`chromeCss` as the page chrome, and the
  trail lowered into the engine's `Scene`
  (`src/domain/usecase/scene.ts`) which the engine's own `crumbsOf` folds
  into the breadcrumb rail.
- The hand-built renderer retired as planned: the `.columns`/`.column`
  shell, its h2 headers, and `domain/model/Palette.ts` (the hand palette)
  are gone — the engine theme is the one color vocabulary.
- The PoC depth measurement was re-run against the served page: 9 columns
  deep at 1280×800 the body's scrollWidth stays 1280 while the strip's row
  scrolls 4457px internally; in the PoC's 420×640 frame, 420 vs 4877.
  Depth does not consume the viewport, as measured.
- The npx smoke now proves a REGISTRY consumer resolves the published
  engine: the packed tarball installs into a scratch tree and the bin runs
  under node, bun, and deno with plggmatic@0.2.0 resolved from the
  registry (under the smoke's ADR-0005 scoped override until the release
  clears the min-release-age floor on 2026-07-24).
