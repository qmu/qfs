// The trail, lowered into the plggmatic engine's `Scene` — the typed
// junction of the mission's ONE pipeline (配管は一本).
//
// The engine's renderers fold a `Scene`; this module is the deterministic
// generator that feeds it from what the viewer resolved for each trail stop:
// the corpus becomes the `MenuLevel`, a document becomes a `DetailLevel`, and
// a qfs path's default view (`lowerToDefaultView`, the describe lowering)
// becomes a `ListLevel` whose rows are exactly the CONTAINMENT links — the
// navigable truth, not the display table. Richer manifest generators (the
// markdown collection path's, later an LLM's) sit BESIDE these functions and
// feed the same `Scene` (workaholic:design / sacrificial-architecture; ADR
// 0002's second amendment records the engine adoption).
//
// Levels here follow the engine's own `back` semantics (its scene builder
// does the same): a level's `back` is the address at which the PREVIOUS
// level is the deepest — which is what makes `crumbsOf` produce the
// prefix-closed crumb trail, and what the strip's column headers use as
// their collapse link. Prefix closure is not re-implemented here; it is
// inherited from the address (docs/adr/0007).
import {
  type SoftStr,
  type Option,
  none,
  some,
  box,
  matchOption,
  fromNullable,
} from "plgg";
import {
  type Scene,
  type Level,
  type MenuLink,
  type RowLink,
  makeRow,
} from "plggmatic";
import { type DefaultView } from "#qfs-viewer/domain/model/Describe";

/**
 * The root column's openable things as the engine's root `MenuLevel`.
 *
 * `entries` is what the reader can OPEN from the root — the page of documents,
 * the declared resources, and the query paths qfs declares (axis 1 of
 * domain/model/Declaration.ts). It is deliberately NOT everything the root
 * column shows: the connections axis (axis 2) is a VIEW, not navigation, and
 * giving it entries here would fuse the two axes in the Scene even while the
 * screen kept them apart. The facets, the pager and the error report are the
 * root body's controls over this list, not entries of it.
 */
export const rootLevel = (
  title: SoftStr,
  entries: ReadonlyArray<MenuLink>,
): Level => box("MenuLevel")({ title, entries });

/**
 * An open document as a `DetailLevel`: the path is the
 * document's identity here (Query.ts says why there is no
 * title), so it is both the level's title and its row.
 */
export const docLevel = (
  path: SoftStr,
  back: Option<SoftStr>,
): Level =>
  box("DetailLevel")({
    collection: "docs",
    title: path,
    back,
    row: some(makeRow(path, path)),
    fields: [],
    actions: [],
  });

/**
 * A declared resource as a `ListLevel`. Its rows are a
 * live table with no navigation of their own (row
 * selection is a strategy-owned open question), so the
 * level carries the column's load truth — its error, when
 * qfs answered one — and no rows.
 */
export const resourceLevel = (
  label: SoftStr,
  back: Option<SoftStr>,
  error: Option<SoftStr>,
): Level =>
  box("ListLevel")({
    collection: label,
    title: label,
    back,
    query: none(),
    rows: [],
    loading: false,
    error,
    actions: [],
  });

// A row's display label: its first cell, unless that cell
// is empty — the child path then names the row, because a
// blank link is not a link a person can read.
const rowLabel = (
  cells: ReadonlyArray<SoftStr>,
  child: SoftStr,
): SoftStr =>
  matchOption<SoftStr, SoftStr>(
    () => child,
    (cell) => (cell === "" ? child : cell),
  )(fromNullable(cells[0]));

/**
 * A walked qfs path's default view as a `ListLevel` — the
 * describe lowering feeding the engine's Scene. The
 * level's rows are ONLY the rows that navigate (a
 * contained child is a segment append, `hrefOfChild`);
 * the remaining cells are display data the strip's table
 * body shows, not links the Scene should invent.
 */
export const qfsLevel = (
  view: DefaultView,
  back: Option<SoftStr>,
  hrefOfChild: (child: SoftStr) => SoftStr,
): Level =>
  box("ListLevel")({
    collection: view.path,
    title: view.path,
    back,
    query: none(),
    rows: view.rows.flatMap(
      (r): ReadonlyArray<RowLink> =>
        matchOption<
          SoftStr,
          ReadonlyArray<RowLink>
        >(
          () => [],
          (child) => [
            {
              row: makeRow(
                child,
                rowLabel(r.cells, child),
              ),
              href: hrefOfChild(child),
              active: false,
            },
          ],
        )(r.child),
    ),
    loading: false,
    error: none(),
    actions: [],
  });

/**
 * A qfs column that could not resolve — describe or read
 * answered an error — still gets a level, because the URL
 * named it and the Scene owes the reader an answer about
 * it (the same honesty rule the column body follows).
 */
export const qfsErrorLevel = (
  path: SoftStr,
  back: Option<SoftStr>,
  message: SoftStr,
): Level =>
  box("ListLevel")({
    collection: path,
    title: path,
    back,
    query: none(),
    rows: [],
    loading: false,
    error: some(message),
    actions: [],
  });

/**
 * The whole strip as ONE `Scene`: the engine's renderer
 * seam, assembled root→leaf. Nothing is pending and
 * nothing confirms — the strip is server-rendered, so the
 * Scene is always the settled one.
 */
export const sceneOf = (
  title: SoftStr,
  levels: ReadonlyArray<Level>,
): Scene => ({ title, levels, confirm: none() });
