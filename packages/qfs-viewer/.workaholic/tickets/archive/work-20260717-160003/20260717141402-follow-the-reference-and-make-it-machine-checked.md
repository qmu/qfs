---
created_at: 2026-07-17T14:14:02+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission: qfs-viewer-mvp
---

# Follow the reference faithfully — and make "follows it" machine-checked

## Overview

From today's design discussion. The developer:

> この qfs-viewer ももっとそのリファレンシャルをよく追いかけるものでなくては
> ならない

The reference is plggmatic's guide + example app (**READ-ONLY** at
`/home/ec2-user/projects/plggmatic` — never modify it). Its canonical
statement, `packages/site/multi-column.md:6-9`:

> the **multi-column renderer** projects that `Scene` into the
> panes-expanding-rightward arrangement: the menu is a `navigation` column,
> each drilled-into list a `complementary` column, and the selected item's
> detail the `main` column, with a sticky `colHead` close link per column and
> a breadcrumb trail above.

PR #9 (merged, `a22775d`) landed the engine strip via published
`plggmatic@0.2.0`: engine `row`/`column`/nav-aside-main panes, the engine's
sticky `colHead` per column, `schemeCss`/`metricCss`/`chromeCss`, and
`src/domain/usecase/scene.ts` lowering the trail into the engine's `Scene`
(corpus → MenuLevel, doc → DetailLevel, describe DefaultView → ListLevel),
with the engine's own `crumbsOf` folding the breadcrumb.

This ticket does two things: **(1) name the divergences concretely**, and
**(2) settle whether conformance is machine-checked** — today a human reads
both.

## (1) The divergences — read, not guessed

Read: the reference's `packages/plggmatic/src/Render/usecase/multiColumn.ts`
and `packages/site/multi-column.md`, against this repo's
`entrypoints/columns.ts` and `domain/usecase/scene.ts`.

### D1 — `multiColumn` is never called. The viewer re-implements it.

`columns.ts:88-99` imports `row, column, navPane, mainPane, asidePane,
colHead, breadcrumb, crumbsOf` and assembles the arrangement by hand. The
reference's `multiColumn` (`Render/usecase/multiColumn.ts:28-38`) imports
**the same primitives from the same modules** and composes them. We are
hand-rolling the thing the engine already ships.

`multi-column.md:64-66` addresses this directly:

> The `Parts` escape hatch on the combinators remains for *other* layouts the
> renderer does not cover — but **the multi-column arrangement is no longer
> the consumer's to assemble.**

And `multi-column.md:44` names the exact history we have re-enacted: "Before
the scheduler, the column-oriented pattern lived once as ~250 lines of
hand-written *app* code ... Ticket 10 lifted all of it into the design
system."

**This is not packaging-blocked — verified.** `multiColumn` (and
`multiColumnWith`, `crumbsOf`, `singleColumn`) are exported from the
**published** `plggmatic@0.2.0` this package already depends on:
`node_modules/plggmatic/dist/index.d.ts:26`. The viewer could call it today.

### D2 — Landmark roles are position-driven; the reference says kind-driven.

`columns.ts:252`:

```ts
const paneOf = deepest ? mainPane : asidePane;
```

The reference (`multiColumn.ts:321-410+`) matches on the **level kind**:
`menuLevel$()` → `navPane` (`basis("220px")`), `listLevel$()` → `asidePane`
(`basis("300px")`), `boardLevel$()`/`detailLevel$()` → `mainPane` (`fluid`).
`multi-column.md:52-54` states the rule:

> `multiColumn` composes the `row`/`column`/`pane` combinators from the
> scheduled `Scene` — **landmark roles come from the level kind, never
> hardcoded.**

**This is observable, not stylistic.** When the deepest column is a
`ListLevel` (a qfs path's default view with no document open — a normal state
for this viewer), we render it `main`; the reference would render it
`complementary`, leaving the page with **no `main` landmark at all**.

**Do not blindly "fix" this one.** Our `columns.ts:242` comment states a
deliberate rationale — "Landmarks stay honest: the deepest column is the
page's `main`" — and a page with zero `main` landmarks is an accessibility
defect. Our rule guarantees exactly one `main`; the reference's does not.
This divergence may be an **improvement that belongs upstream in the
reference**, not a bug to correct here. It must be *settled and recorded*,
which is the point — what is not allowed is that it stays undeclared, which
is what it is today.

### D3 — The declaration/scheduler layer is absent.

The reference's canonical path (`multi-column.md:24-41`) is
`declare({...collections})` → `schedule(app)` → `scheduled.scene(model)` →
`multiColumn(scene)`. The scheduler derives the `Model`, `Msg` union,
`update`, URL codec, and `Scene` **from a declaration**.

We have none of it: `scene.ts` hand-builds the `Scene` (`sceneOf`,
`corpusLevel`, `docLevel`, `resourceLevel`, `qfsLevel`, `qfsErrorLevel`).
Nothing in `src/` calls `declare`, `collection`, `menu`, or `schedule`
(grep: the only `schedule` hits are `reload.ts`'s unrelated timer seam).

### D4 — Column widths diverge.

Reference: `220px` menu / `300px` list / `fluid` detail+board — the width is
a function of the kind. Ours: `basis("22rem")` for the corpus
(`columns.ts:1193`), and a uniform `basis("32rem")` for **every** other
column (`columns.ts:254`). Nothing is `fluid`; the detail column never
expands to fill.

### D5 — One level kind unused.

The engine has four (`menuLevel$`, `listLevel$`, `boardLevel$`,
`detailLevel$`); `scene.ts` produces three. `BoardLevel` may be legitimately
unused — but that is a claim to record, not a silence.

### The honest tension behind D1 (do not skip this)

`MultiColumnOptions` requires `mapMsg: (msg: SchedulerMsg) => Msg` — the
renderer is built for the interactive scheduler's Msg loop. **This viewer is
server-rendered**: every call site is `<never>` (`navPane<never>`,
`colHead<never>`, `breadcrumb<never>`), and `scene.ts:167` says so — "the
strip is server-rendered, so the Scene is always the settled one". An SSR
page with no client runtime has no `Msg` to map. That is a real reason the
hand-assembly happened, and the ticket must not pretend D1 is a one-line
swap.

The counter-evidence, also real: `multiColumnWith` + `MultiColumnOptions`
already carry `omitBreadcrumb`, `headerLinks`, and `extraColumns` — a
customization surface built for exactly this kind of need. **Settling whether
an SSR consumer can call `multiColumn` (and if not, what the engine owes it)
is this ticket's central question.** The answer may be an upstream change,
not a local one.

## (2) Make it machine-checked

Today a human reads both trees. PR #9 measured *our* page — "9 columns deep,
body scrollWidth constant at 1280/420 while `.pm-row` scrolls 4457–4877px
internally, `pm-colhead` sticky" — but measured only ours. **A measurement
applied to the reference too turns "follows the reference" from a reading
into a test.**

### Recommended shape

One **conformance probe**: a single headless-browser script, parameterised by
URL, run against BOTH the reference demos and our served page, asserting the
same invariants and diffing the results:

| invariant | why it is the right level |
| --- | --- |
| landmark role **per column kind** (`nav` on menu, `complementary` on list, `main` on detail) | catches D2 — the divergence a human found by reading |
| exactly-one-`main` (or zero, once D2 is settled) | pins whichever answer D2 lands on |
| body `scrollWidth` constant as depth grows; `.pm-row` scrolls internally | PR #9's measurement, now applied to both |
| `pm-colhead` computes `position: sticky` | the reference's stated chrome |
| breadcrumb trail is prefix-closed against the crumb hrefs | `crumbsOf`'s contract |

The demos are the right target: `packages/site/dist/example/demo{1,2,3}.html`
are static, self-contained (an HTML shell + one bundled module, ~580–650KB),
and demo1 is "the eight-section menu declared from scratch and **rendered by
the scheduler**" — i.e. the canonical `declare`→`schedule`→`multiColumn` path
in action. `pm-col`, `pm-colhead`, `pm-row` hooks are present in the bundle.
They render client-side, so the probe needs a real browser — which is what
PR #9 already used (`~/.cache/ms-playwright/chromium-1228/...`).

### The cost — state it plainly

**The reference must stay reachable and buildable for this test to run.**
plggmatic is on the retirement path, and the reference is about to be rescued
into qmu-co-jp as a separate Worker (a ticket is filed there). So this probe
**takes a dependency on the rescue's artifacts-vs-source decision — currently
the developer's open call**:

- **Artifacts-only rescue** (ship `dist/example/*`): sufficient for this
  probe — the demos are already built, self-contained, and need no toolchain.
  Cheapest, and it makes the probe a pure DOM comparison.
- **Source rescue**: lets the probe rebuild when the engine changes, at the
  cost of qmu-co-jp inheriting a build.

**The limit of a DOM probe, recorded honestly:** it pins *rendered shape*,
not *call structure*. **D1 is invisible to it** — if our hand-assembly
happens to emit the same DOM, the probe passes while we still re-implement
`multiColumn`. D3 likewise. A DOM probe would catch D2, D4, and regressions;
it would not catch "you rebuilt the renderer". If D1/D3 are to be enforced,
that is a source-level check (an import/call assertion), not a browser one.
Do not oversell the probe as covering the divergence that prompted this
ticket.

## Implementation Steps

1. **Record the divergence table** (D1–D5) as a decision document, each entry
   settled one of three ways: conform to the reference, diverge deliberately
   *with the reason recorded*, or push the change upstream. D2 is the one
   most likely to land "upstream", and it must not be silently "fixed".
2. **Settle D1** — whether an SSR consumer can call `multiColumn`/
   `multiColumnWith`, and if not, what the engine owes it. This is the
   central question; the other divergences partly dissolve if it resolves
   toward calling the renderer.
3. **Build the conformance probe** — one script, two URLs, the invariant
   table above, exit code 0/1. It runs against the reference *and* us.
4. **Wire it to the rescued reference's URL** once the rescue lands; until
   then it runs against the local read-only checkout. Record which
   artifacts-vs-source answer the probe assumed.

## Policies

- `workaholic:implementation` / `policies/objective-documentation.md` — D2 is
  a deliberate, *documented-in-code* divergence from a *documented* reference
  rule, and nobody recorded that it diverges. Both cannot be canonical; the
  disagreement is the artifact to record.
- `workaholic:design` / `policies/sacrificial-architecture.md` — the
  hand-built assembly was the sacrificial skin; `multi-column.md:44-50`
  records the engine having already absorbed exactly this ~250 lines once.
  Re-growing it here is the regression the reference warns against.
- `workaholic:planning` / `policies/accessibility-first.md` — the landmark
  question is not cosmetic. "No `main` on the page" and "two `main`s" are
  both real defects; the probe is what makes the answer checkable instead of
  incidental.
- `workaholic:implementation` / `policies/coding-standards.md` — level kinds
  are a closed union; a kind-driven `match` is a compile-time exhaustiveness
  win that `deepest ? a : b` throws away.
- `workaholic:operation` — the probe's dependency on a reachable reference is
  an operational commitment (the rescued Worker must stay up for the test to
  run), not just a test-authoring choice.

## Key Files

- `src/entrypoints/columns.ts` — `columns.ts:88-99` (the primitive imports),
  `:252` (`paneOf = deepest ? mainPane : asidePane`, D2), `:254` / `:1193`
  (the widths, D4), `:1400` (`breadcrumb(crumbsOf(scene))`).
- `src/domain/usecase/scene.ts` — the hand-built Scene (D3); `sceneOf:167`
  (the SSR "settled Scene" note behind D1's tension).
- Reference only, **never modified** —
  `/home/ec2-user/projects/plggmatic/packages/site/multi-column.md` (the
  canonical statement), `packages/plggmatic/src/Render/usecase/multiColumn.ts`
  (the kind-driven `match`), `packages/site/dist/example/demo{1,2,3}.html`
  (the probe's targets), served at `plggmatic-guide.qmu.dev`.
- `packages/qfs-viewer/node_modules/plggmatic/dist/index.d.ts:26` — proof
  `multiColumn`/`multiColumnWith` ship in the published 0.2.0.

## Quality Gate

### Acceptance Criteria

- Every divergence D1–D5 is settled and recorded with its reason — conform,
  diverge deliberately, or upstream. **No divergence remains undeclared.**
- The conformance probe runs against the reference AND qfs-viewer with the
  same invariant set, and exits non-zero on a mismatch.
- The probe's coverage limit is recorded: it does **not** catch D1/D3.
- The rescue's artifacts-vs-source assumption the probe depends on is named
  explicitly (it is the developer's open call — this ticket records the
  dependency, it does not decide it).
- If D2 lands as "conform", a page whose deepest column is a `ListLevel` has
  no `main`, and that is asserted deliberately, not discovered.

### Verification Method

```sh
# the probe, run against BOTH — the point of the ticket
./scripts/conformance-probe.sh http://localhost:4100/resolve/...
./scripts/conformance-probe.sh file:///home/ec2-user/projects/plggmatic/packages/site/dist/example/demo1.html

# D1, mechanically: do we call the renderer, or rebuild it?
grep -rnE 'multiColumn|multiColumnWith' packages/qfs-viewer/src/   # expect a hit once D1 is settled toward conforming

# D2, mechanically: is the role a function of the kind?
grep -n 'deepest ? mainPane' packages/qfs-viewer/src/entrypoints/columns.ts

./scripts/check-all.sh
```

### Gate

- `./scripts/check-all.sh` exits 0.
- **The probe must fail at least once before it passes.** A conformance test
  that has never gone red against a known divergence (D2 is sitting there
  today) is not evidence of conformance — run it against current `main`
  first and watch it catch D2, then fix forward.

## Considerations

- **Never modify `/home/ec2-user/projects/plggmatic`.** It is read-only and
  on the retirement path; the rescue into qmu-co-jp is a separate ticket in
  that repo.
- **This ticket does not decide artifacts-vs-source.** That is the
  developer's open call on the rescue. Record which answer the probe assumes
  and adapt; do not force the rescue's hand from here.
- **D2 may be a reference bug, not ours.** Resist the reflex to make the
  viewer match a rule that produces a page with no `main` landmark. "Follow
  the reference" means the disagreement gets adjudicated and written down —
  not that the reference is automatically right.
- **The probe cannot see D1, which is the divergence the developer's words
  are actually about.** Guard against declaring victory when the DOM matches:
  a green probe over a hand-rolled renderer is exactly the comfortable,
  wrong answer here.
