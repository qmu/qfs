# 0010 — Following the plggmatic reference: five divergences, settled

**Status:** Accepted (2026-07-17)
**Ticket:** 20260717141402-follow-the-reference-and-make-it-machine-checked.md
**Mission:** qfs-viewer-mvp

The developer, at the 2026-07-17 design discussion:

> この qfs-viewer ももっとそのリファレンシャルをよく追いかけるものでなくては
> ならない

The reference is plggmatic's guide + example app
(`/home/ec2-user/projects/plggmatic`, READ-ONLY). This ADR settles the five
places this viewer diverges from it. Each is settled one of three ways —
**conform**, **diverge deliberately**, or **upstream** — because the thing
that was not allowed was that a divergence stays *undeclared*, which is what
all five were until now.

## Decision, in one table

| # | Divergence | Settlement |
| --- | --- | --- |
| D1 | `multiColumn` is never called; the viewer re-implements it | **Diverge + upstream** — the engine cannot render this viewer's column bodies today |
| D2 | Landmark role position-driven, not kind-driven | **Both rules were wrong; a third is now implemented** and belongs upstream |
| D3 | No `declare`/`schedule` layer | **Diverge deliberately** — an SSR viewer has no update loop to derive |
| D4 | Uniform `32rem` columns vs kind-driven `220px`/`300px`/fluid | **Diverge deliberately** — our columns hold long-form prose |
| D5 | `BoardLevel` unused | **Recorded as legitimately unused** |

## D1 — We re-implement `multiColumn`. We must, today.

**The tension the ticket flagged dissolves under measurement.** The ticket
worried that `multiColumn` requires `mapMsg: (SchedulerMsg) => Msg` while this
viewer is SSR (`<never>` throughout). Both halves of that turn out not to
bind:

- **`multiColumn(scene)` takes only a scene.** It supplies `mapMsg` itself, as
  identity (`multiColumn.ts:107-112`). The `mapMsg` requirement is on
  `multiColumnWith`, not on `multiColumn`.
- **SSR does not require `Html<never>`.** plgg-view's renderer is
  `renderToString: <Msg>(node: Html<Msg>) => SoftStr` — generic in `Msg`, and
  it "drops event handlers" by contract. `Html<SchedulerMsg>` renders to a
  string exactly as well as `Html<never>` does.

So `renderToString(multiColumn(scene))` typechecks and runs. The `<never>` in
this file is a *choice*, not a constraint SSR imposed. **The ticket's stated
reason for the hand-assembly is not the real one.**

**The real blocker is deeper, and it is not packaging.** `Scene` is
`presentation-neutral data` by design (`Schedule/model/Scene.ts`), and
`multiColumn` derives each column's **body** from it: `menuNav(entries)`,
`rowList(rows)`, `detailFields(fields)`, `tileGrid(tiles)`. This viewer's
column bodies are none of those. They are:

- a **rendered markdown document** (`raw()` over plgg-md's HTML) — the product,
- a links table carrying each link's section path,
- a resources section,
- a `GET` form for entering an arbitrary qfs path.

None of that is expressible as a `Level`. A `DetailLevel` carries
`row: Option<Row>` and `fields: ReadonlyArray<DetailField>` — typed field
values, not markup. **Calling `multiColumn(scene)` today would render
`detailFields([])` where the document belongs**, and the viewer would show
"Not found" in place of every document it exists to show.

The customization surface does not close the gap. `extraColumns` and
`afterMenu` append **app-owned columns beside** the scene's levels; neither
supplies **the body of a scene level**. Routing every column through
`extraColumns` would emit `column([fluid], [mainPane(...)])` for each — every
column a `main` landmark, and the level kinds erased. That is not adoption; it
is the same re-implementation with worse semantics.

**Settlement: diverge, and the change belongs upstream.** What the engine owes
an SSR consumer with rich bodies is a **per-level body seam** — e.g.
`renderBody?: (level: Level, depth: number) => Option<ReadonlyArray<Html<Msg>>>`
on `MultiColumnOptions` — that overrides the derived body while the renderer
keeps what it is actually good at: the row/column/pane assembly, the
kind→role mapping, the widths, the sticky `colHead`, and the breadcrumb. With
that seam this viewer calls `multiColumnWith` and D1/D3/D4 collapse together.

`multi-column.md:64-66` says "the multi-column arrangement is no longer the
consumer's to assemble." That is true for a consumer whose columns are the
scheduler's own rows and fields. It is not yet true for a consumer whose
columns are documents — and the reference has no such consumer, which is why
the gap was never found. **This is the reference's gap, recorded here rather
than worked around silently.**

## D2 — Kind-driven vs position-driven: both rules emit pages with no `main`

The reference maps kind→role (`multiColumn.ts:321-410`): `menuLevel$()` →
`navPane`, `listLevel$()` → `asidePane`, `boardLevel$()`/`detailLevel$()` →
`mainPane`. `multi-column.md:52-54`: "landmark roles come from the level kind,
never hardcoded."

This file had `paneOf = deepest ? mainPane : asidePane`, with the rationale
"Landmarks stay honest: the deepest column is the page's `main`".

**The rationale was false, and it was false on the most-visited page.**
Measured before this change, over the rendered HTML:

| page | `<main>` | `<nav>` | `<aside>` |
| --- | --- | --- | --- |
| `/` (root) | **0** | 1 | 0 |
| `/resolve/<doc>` | 1 | 1 | 0 |

The corpus column **bypassed** `shellColumn` and hardcoded `navPane`, so at
depth 0 — the only column on screen — the page had **zero `main` landmarks**.
The ticket's premise ("Our rule guarantees exactly one `main`; the reference's
does not") was therefore wrong about our own code. The reference's rule is
main-less whenever nothing is drilled into; ours was main-less at the root.
**Neither rule guaranteed a `main`.** Both are accessibility defects
(`workaholic:planning` / `accessibility-first`).

**Settlement: implement a third rule, and push it upstream.**

1. The **deepest** column is `main`, whatever its kind — it is what the URL
   named and what the reader came for, which is what `main` means.
2. Every **shallower** column takes the role its **level kind** declares
   (menu → `nav`, list/detail/board → `complementary`) — the reference's own
   rule, restored via a `match` on the closed union, so a fifth level kind is
   a compile error rather than a column that silently picks a landmark.

Rule 2 is the reference's. Rule 1 is the deliberate divergence, and it is what
makes "exactly one `main`, at every depth" true rather than aspirational.
Measured after:

| page | `<main>` | `<nav>` | `<aside>` |
| --- | --- | --- | --- |
| `/` (root) | **1** | 0 | 0 |
| `/resolve/<doc>` | 1 | 1 | 0 |
| `/resolve/<doc>,<doc>` | 1 | 1 | 1 |

**This belongs upstream**: `multiColumn` should take the same two rules, and
the reference's demo1 — an eight-section menu at the root with nothing drilled
— renders no `main` today for exactly the reason our root did.

## D3 — No `declare`/`schedule` layer

The reference's canonical path is `declare({...collections})` →
`schedule(app)` → `scheduled.scene(model)` → `multiColumn(scene)`. The
scheduler derives `Model`, the `Msg` union, `update`, and the URL codec from a
declaration.

**Settlement: diverge deliberately.** What the scheduler derives is the
machinery of a **client-side Msg loop**: an `update` to fold messages into a
model, and a URL codec to reflect that model into the address bar. This viewer
has no such loop and wants none — `docs/adr/0007` records why the URL *is* the
state rather than a report of it, and `entrypoints/columns.ts:9-17` records
that server rendering is what makes "a reload restores the same columns" true
instead of merely intended. There is no model to derive an `update` for, and
the URL codec is the thing this package deliberately owns.

D3 is also **downstream of D1**: adopting the scheduler buys nothing while the
renderer it feeds cannot draw our column bodies. If D1's upstream seam lands,
D3 is worth revisiting; until then it is a layer with no work to do.

## D4 — Column widths

Reference: `basis("220px")` menu / `basis("300px")` list / `fluid` detail and
board — the width is a function of the kind. Ours: `basis("22rem")` for the
corpus, uniform `basis("32rem")` for every other column, nothing `fluid`.

**Settlement: diverge deliberately.** The reference's widths are sized for
what the reference's columns hold — a row list and a field-per-line record. A
`300px` column is a correct size for a list of names and a wrong one for a
rendered markdown document with code blocks and tables, which is what every
non-corpus column here holds. The uniform `32rem` is a measure cap for prose,
not an oversight.

`fluid` is separately declined: it makes the last column absorb the remaining
row width, which fights the one behaviour this strip is built to guarantee —
depth grows the strip's own scroll width, never the page body's
(`docs/plggmatic-semantics/poc-findings.md`).

If D1's body seam lands, the width becomes the renderer's again and this
divergence needs re-arguing with it — the seam should let a consumer state the
measure, or the engine's kind→width map should learn that a detail column can
hold prose.

## D5 — `BoardLevel` unused

The engine has four level kinds; `scene.ts` produces three. **Settlement:
recorded as legitimately unused.** `BoardLevel` is for dashboard-shaped
screens — tiles of label + caption + jump, with "no query and no drill". This
viewer's three stops are a menu (the corpus), a document (detail), and a qfs
path's default view (list). Nothing here is a dashboard, and a tile that
cannot drill is the opposite of a strip whose whole point is drilling. The
kind stays unused until a screen wants it; the `match` in `paneFor` handles it
exhaustively regardless, so it cannot be forgotten.

## The machine check, and what it cannot see

`columns.spec.ts` asserts the D2 contract — exactly one `<main>` at every
depth, the corpus `nav` once something is open, a non-deepest document
`complementary`. It runs inside `./scripts/check-all.sh`. It went **red on the
pre-change tree** (`✗ the root page has exactly one main landmark`) before it
went green, which is the only reason to believe it checks anything.

**It deliberately is not the cross-tree DOM probe the ticket sketched.** Three
reasons, in order of weight:

1. **A probe that diffed our landmarks against the reference's would have to
   go red on D2 — the divergence we chose on purpose.** Its headline invariant
   ("landmark role per column kind") is precisely what we settled *against*.
   It would measure our disagreement and call it a failure.
2. **It cannot see D1 or D3.** The ticket says this itself, and it is the
   decisive point: D1 is a *call-structure* fact, and every DOM assertion
   passes over a hand-rolled renderer that emits the same markup. D1 is what
   the developer's words were actually about, so a green DOM probe would be
   the comfortable wrong answer — conformance theatre over the one divergence
   that matters.
3. **It would take an operational dependency on a retiring reference.** The
   probe needs plggmatic's demos reachable and a headless browser in the gate,
   and plggmatic is on the retirement path — its rescue into qmu-co-jp is the
   developer's open call (artifacts-only vs source). **This ADR records that
   dependency rather than assuming an answer to it**: no check here needs the
   reference to be up.

D1 and D3 are pinned by **this ADR's prose** and by the source grep in the
ticket's Verification Method (`grep -rnE 'multiColumn|multiColumnWith'
packages/qfs-viewer/src/` — no hit, deliberately, until the upstream seam
lands). A source-level import/call assertion is the tool that would enforce
them mechanically; it is not built here because the settlement is "diverge
until upstream moves", and a test pinning "we do NOT call the renderer" would
have to be deleted by the change that fixes it.

## Consequences

- The viewer keeps ~250 lines the engine already absorbed once
  (`multi-column.md:44`), and `workaholic:design` / `sacrificial-architecture`
  is right that re-growing it is a regression. **The regression is recorded,
  not denied**: it persists because the engine cannot draw a markdown document,
  and it retires when D1's seam lands.
- Two upstream asks now exist against plggmatic: the **per-level body seam**
  (D1) and the **landmark rule** (D2). Both are this repo's to file, and both
  are aimed at a reference on the retirement path — so both should ride with
  the rescue rather than land in a repo about to be retired.
- The a11y defect the root page shipped with is fixed and pinned. It was found
  by measuring, not by reading — including reading the comment that asserted
  the opposite.
