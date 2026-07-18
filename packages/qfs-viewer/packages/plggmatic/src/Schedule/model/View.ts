import {
  type SoftStr,
  type Box,
  type Icon,
  box,
  icon,
  pattern,
} from "plgg";

/**
 * A node in the derived view graph — the (view, typed
 * params) core of the screen-structure model (mission
 * decision, 2026-07-12). A `View` names WHAT is being
 * looked at, never how it is arranged: the menu, a
 * collection's list, or a collection row's detail. The
 * legacy declaration vocabulary lowers onto this core
 * (collection → list + detail views, `child` → the
 * navigation between them); the plgg-ir Domain Manifest
 * later lowers onto the same core.
 *
 * A `View` is an ADDRESS, not a resolution: it may name a
 * collection the declaration cannot resolve (a dangling
 * root or child), which downstream folds degrade to an
 * empty level — the same totality URLs get.
 */
export type View =
  | Icon<"MenuView">
  | Box<"ListView", SoftStr>
  | Box<"DetailView", SoftStr>;

/** The menu view (no collection opened). */
export const menuView = (): View =>
  icon("MenuView");

/** A collection's list view. */
export const listView = (
  collection: SoftStr,
): View => box("ListView")(collection);

/** A collection row's detail view. */
export const detailView = (
  collection: SoftStr,
): View => box("DetailView")(collection);

/** Matchers for folding a {@link View}. */
export const menuView$ = () =>
  pattern("MenuView")();
export const listView$ = () =>
  pattern("ListView")();
export const detailView$ = () =>
  pattern("DetailView")();

/**
 * One typed parameter binding addressing a view: the
 * collection a selection belongs to and the selected row
 * id. A navigation target is a `View` plus the bindings
 * of every ancestor selection on the way to it (and, for
 * a detail view, the viewed row's own binding last).
 */
export type Binding = Readonly<{
  collection: SoftStr;
  id: SoftStr;
}>;

/** Constructs a {@link Binding}. */
export const binding = (
  collection: SoftStr,
  id: SoftStr,
): Binding => ({ collection, id });
