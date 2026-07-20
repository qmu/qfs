import {
  type SoftStr,
  type Box,
  type Option,
  box,
  pattern,
} from "plgg";
import {
  type Row,
  type FieldValue,
} from "plggmatic/Declare/model/Row";
import { type Verb } from "plggmatic/Declare/model/Action";

/**
 * A confirmation the renderer must surface as a dialog —
 * the prompt and whether the pending action is
 * destructive (so the renderer can style the confirm
 * button and label it accessibly).
 */
export type ConfirmPrompt = Readonly<{
  prompt: SoftStr;
  destructive: boolean;
}>;

/**
 * A button a renderer draws for an available action, with
 * the declared semantics it needs: the verb (for styling
 * create vs delete) and whether it is destructive.
 */
export type ActionButton = Readonly<{
  id: SoftStr;
  label: SoftStr;
  verb: Verb;
  destructive: boolean;
}>;

/** One list row projected for a renderer. */
export type RowLink = Readonly<{
  row: Row;
  href: SoftStr;
  active: boolean;
}>;

/** One menu entry projected for a renderer. */
export type MenuLink = Readonly<{
  label: SoftStr;
  href: SoftStr;
  active: boolean;
}>;

/**
 * One declared query choice projected for a renderer: the
 * control caption, the closed option set, and the chosen
 * value (`""` when unfiltered).
 */
export type QueryChoiceState = Readonly<{
  id: SoftStr;
  label: SoftStr;
  options: ReadonlyArray<SoftStr>;
  value: SoftStr;
}>;

/** The live query state a renderer draws (if any). */
export type QueryState = Readonly<{
  placeholder: SoftStr;
  text: SoftStr;
  choices: ReadonlyArray<QueryChoiceState>;
}>;

/**
 * One detail field projected for a renderer: the typed
 * value plus, for a `Reference`, the resolved href of the
 * target's CANONICAL address (the cross-link jump — built
 * by the scene, so renderers never see the model). `None`
 * for every non-reference value.
 */
export type DetailField = Readonly<{
  label: SoftStr;
  value: FieldValue;
  href: Option<SoftStr>;
}>;

/**
 * One board tile projected for a renderer: the headline
 * `label`, the supporting `caption` (the row's first
 * field), and the tile's jump — the href of the summarized
 * target's CANONICAL address, resolved by the scene from
 * the row's `Reference` field (`None` when the tile
 * carries no reference). The jump is the tile's ONLY
 * interaction: board rows do not drill.
 */
export type Tile = Readonly<{
  label: SoftStr;
  caption: SoftStr;
  href: Option<SoftStr>;
}>;

/**
 * A position in the drill-down flow, projected to
 * presentation-neutral data — the renderer seam. A closed
 * union: a renderer folds it with an exhaustive `match`,
 * so a new level kind is a compile error at every
 * renderer (tickets 10/11). A `level` is DEPTH, not a
 * column/screen — mode-agnostic (D10). The union grows
 * deliberate variants as target screens demand them (the
 * point-5 decision, 2026-07-12) — never a layout-data
 * generalization.
 *
 * - `MenuLevel` — the root navigation entries.
 * - `ListLevel` — a collection's filtered rows, its query
 *   state, load state, back link, and available actions.
 * - `BoardLevel` — a board collection's tiles (dashboard-
 *   shaped screens): label + caption + jump per tile, no
 *   query and no drill.
 * - `DetailLevel` — a selected row's fields, back link,
 *   and available actions.
 */
export type Level =
  | Box<
      "MenuLevel",
      Readonly<{
        title: SoftStr;
        entries: ReadonlyArray<MenuLink>;
      }>
    >
  | Box<
      "ListLevel",
      Readonly<{
        collection: SoftStr;
        title: SoftStr;
        back: Option<SoftStr>;
        query: Option<QueryState>;
        rows: ReadonlyArray<RowLink>;
        loading: boolean;
        error: Option<SoftStr>;
        actions: ReadonlyArray<ActionButton>;
      }>
    >
  | Box<
      "BoardLevel",
      Readonly<{
        collection: SoftStr;
        title: SoftStr;
        back: Option<SoftStr>;
        tiles: ReadonlyArray<Tile>;
        loading: boolean;
        error: Option<SoftStr>;
      }>
    >
  | Box<
      "DetailLevel",
      Readonly<{
        collection: SoftStr;
        title: SoftStr;
        back: Option<SoftStr>;
        row: Option<Row>;
        fields: ReadonlyArray<DetailField>;
        actions: ReadonlyArray<ActionButton>;
      }>
    >;

/**
 * The full scheduled scene a renderer draws: the app
 * title, the ordered `levels` stack (root→leaf), and any
 * pending confirmation. The whole mode-agnostic truth a
 * renderer needs — no re-derivation from the model.
 */
export type Scene = Readonly<{
  title: SoftStr;
  levels: ReadonlyArray<Level>;
  confirm: Option<ConfirmPrompt>;
}>;

/** Constructs a `MenuLevel`. */
export const menuLevel = (
  title: SoftStr,
  entries: ReadonlyArray<MenuLink>,
): Level => box("MenuLevel")({ title, entries });

/** Constructs a `ListLevel`. */
export const listLevel = (l: {
  collection: SoftStr;
  title: SoftStr;
  back: Option<SoftStr>;
  query: Option<QueryState>;
  rows: ReadonlyArray<RowLink>;
  loading: boolean;
  error: Option<SoftStr>;
  actions: ReadonlyArray<ActionButton>;
}): Level => box("ListLevel")(l);

/** Constructs a `BoardLevel`. */
export const boardLevel = (l: {
  collection: SoftStr;
  title: SoftStr;
  back: Option<SoftStr>;
  tiles: ReadonlyArray<Tile>;
  loading: boolean;
  error: Option<SoftStr>;
}): Level => box("BoardLevel")(l);

/** Constructs a `DetailLevel`. */
export const detailLevel = (l: {
  collection: SoftStr;
  title: SoftStr;
  back: Option<SoftStr>;
  row: Option<Row>;
  fields: ReadonlyArray<DetailField>;
  actions: ReadonlyArray<ActionButton>;
}): Level => box("DetailLevel")(l);

/** Matchers for folding a {@link Level}. */
export const menuLevel$ = () =>
  pattern("MenuLevel")();
export const listLevel$ = () =>
  pattern("ListLevel")();
export const boardLevel$ = () =>
  pattern("BoardLevel")();
export const detailLevel$ = () =>
  pattern("DetailLevel")();
