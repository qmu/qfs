# plggmatic

> **UNSTABLE** - Experimental study work. A package of the
> [qfs-viewer repository](../../README.md).

> **Provenance.** This package was ported from the retired
> [`qmu/plggmatic`](https://github.com/qmu/plggmatic) repository
> (`packages/plggmatic`) on 2026-07-16 (HQ ticket 20260716212002). Its git
> history stays in the retired repo; the reference app (`plggmatic-example`)
> remains there pending its integration into qmu-co-jp's design-pattern
> article. Body text below predates the move, so monorepo-relative links
> (`packages/site/`, `packages/plggmatic-example/`, sibling plgg packages) refer
> to the old homes.

A **column-oriented UI design framework** on the plgg family: a typed light/dark
**color scheme** (roles resolved through `var(--pm-*)` custom properties, so one
`dark` class reschemes the whole tree), **layout combinators** (`row` / `column`
/ `pane`) composed like element builders take `(parts, children)`, and
**fundamental components** as pure `(props) => Html<Msg>`. Styling is data —
atoms composed through [plgg-view](../plgg-view/)'s `style_`, gathered by
`collectCss`. No layout config object and no runtime to boot.

The vocabulary is one word per concept: **Column**, **Pane**, **Alignment**,
**Token**, **Scheme**. The design system is emergent — seeded minimal, one
recorded rule per component.

This is the UI design framework, canonical in this monorepo since `6d7a832`
(decision D13). See the documentation site in [`packages/site/`](../site/) and
the workbench demo in [`packages/plggmatic-example/`](../plggmatic-example/).

**Disambiguation — "plggmatic" has two historical meanings.** (a) A retired
2026-06/07 _app-framework facade_, absorbed into
[`plggpress/src/framework/`](../plggpress/src/framework/) in `31fdee9`; archived
tickets and their "rewire map" tables describe _that_ plggmatic — do not apply
them to this package. (b) _This_ package — the UI design framework — re-imported
at `6d7a832` and canonical here (D13). This note is the single source of the
distinction; other sites link here rather than repeating it.

## The declarative scheduler (framework half)

plggmatic is not only the design system — its essence is **declarative
definition of menus, data lists/details, actions, search, and flows, from which
a UI program is automatically _scheduled_** (D1). You write a **declaration** as
data; `schedule(...)` derives the whole plgg-view program except the view — the
`Model`, the `Msg` union, a pure `update`, and a total URL codec — plus a typed
`Scene` a renderer draws. The vocabulary is **mode-agnostic** (D10): no
declaration or derived type names a column, pane, drawer, or screen. Renderers
(tickets 10/11) project the derived level stack into a display.

```ts
import {
  schedule,
  declare,
  menu,
  menuEntry,
  collection,
  sync,
  query,
  makeRow,
} from "plggmatic";
import { application } from "plgg-view/client";

// a declaration — pure data, performs nothing
const app = declare({
  title: "Field Notes",
  menu: menu([menuEntry("Sections", "sections")]),
  collections: [
    collection<Section>({
      id: "sections",
      title: "Sections",
      toRow: (s) => makeRow(s.id, s.label),
      source: sync(() => sections),
      child: "notes",
      query: query("Filter"),
    }),
    collection<Note>({
      id: "notes",
      title: "Notes",
      toRow: (n) =>
        makeRow(n.id, n.title, [/*…*/]),
      source: sync((path) => notesFor(path[0])),
    }),
  ],
});

// schedule derives init/update/onUrlChange/toUrl/scene;
// a renderer supplies the missing `view`
const s = schedule(app);
application({
  ...s,
  view: (m) => render(s.scene(m)),
})(document.getElementById("root")!);
```

### Two modes, one declaration (D10)

The scheduled `Scene` is **mode-agnostic**: the multi-column renderer draws it
as panes expanding rightward, the single-column renderer as one operation per
screen, and `renderMode(mode)(scene)` dispatches between them. A consumer holds
the `Mode` _beside_ the scheduled model (never in it), and a flip mid-flow is
loss-free by construction — same flow position, selection, query, confirmation,
and URL:

```ts
import {
  schedule,
  renderMode,
  toggleMode,
  type Mode,
} from "plggmatic";

const scheduled = schedule(app);

// the consumer's model = scheduled model + a Mode
type Model = {
  scheduled: ScheduledModel;
  mode: Mode;
};

const view = (model: Model) =>
  renderMode(model.mode)(
    scheduled.scene(model.scheduled),
  );
// a "toggle mode" button dispatches toggleMode(model.mode)
```

The vocabulary — **Resource/Collection** (sync or async through one shape),
**Menu**, **Row** (the list/detail projection), **Action** (create/update/delete
with confirmation-as-data), **Query**, and the **Flow** graph (menu roots +
each collection's `child`) — is a set of closed unions consumed with exhaustive
`match`, so adding a variant is a compile error at every interpreter. Effects
are `Cmd` data: an `async` source read and an `Action`'s verb are _returned_ by
`update`, never run by it. See the runnable proof-of-value — the reference app
is itself a declaration scheduled and drawn by the multi-column renderer —
in [`packages/plggmatic-example/`](../plggmatic-example/) (`src/declaration.ts` +
`src/app.ts`; the form components have their own showcase in `src/forms/`), and
the design record at
[`.workaholic/specs/20260704-plggmatic-scheduler-design.md`](../../.workaholic/specs/20260704-plggmatic-scheduler-design.md).

## Palette override & scheme persistence

The color system is a closed role×variant matrix (see the
[color-scheme docs](../site/color-scheme.md)) with a **monochrome default**.
Two consumer seams:

- **Override** — `defaultPalette` is the shipped monochrome palette; an app
  validates its own brand colors with `asPalette` (`unknown` → `Result`, a
  missing scheme/token/bad-hex is an `Err` naming the path) and emits the CSS
  with `schemeCssOf`. `contrastRatio` runs the same WCAG math the phase-1 gate
  uses so an override can be audited. Atoms and `var(--pm-*)` are untouched.
- **Persistence** — the contract is framework-owned: one storage key
  `appearanceStorageKey` = **`vp-appearance`** (preserved per D16), one
  mechanism **`html.dark`**, a no-FOUC `appearanceInitScript` +
  `injectAppearanceScript`, a pure `decideScheme`, and an `applyScheme` effect
  helper. Every consumer schemes identically — no per-app key drift.
