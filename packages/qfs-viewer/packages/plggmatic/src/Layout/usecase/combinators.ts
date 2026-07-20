import { type SoftStr } from "plgg";
import { type Html, el } from "plgg-view";
import {
  type Styles,
  type Variant,
  style_,
  flex,
  flexCol,
} from "plggmatic/styleEntry";
import {
  type PaneRole,
  landmarkTag,
} from "plggmatic/Layout/model/pane";

/**
 * What a combinator accepts as its style slot: exactly
 * `style_`'s parts (class hooks, atom groups, variants).
 * The combinator merges its own base parts and the
 * consumer's into ONE `style_` call — `style_` is the
 * sole class authority, so two calls on one element
 * would clobber each other.
 */
export type Parts = ReadonlyArray<
  SoftStr | Styles | Variant
>;

/**
 * The screen as a horizontal strip of columns — the
 * column-oriented pattern's outermost piece. Purely
 * compositional: `row` contributes only `display:flex`
 * and its `pm-row` hook; height, snapping, and collapse
 * are the consumer's parts and stylesheet. **Recorded
 * rule**: layout combinators take `(parts, children)`
 * like element builders take `(attributes, children)` —
 * options are style atoms you compose, never fields on a
 * config object.
 */
export const row = <Msg>(
  parts: Parts,
  children: ReadonlyArray<Html<Msg>>,
): Html<Msg> =>
  el(
    "div",
    [style_("pm-row", flex, ...parts)],
    children,
  );

/**
 * One vertical track of the row. Contributes only the
 * column flow (`flex-direction:column`) and its `pm-col`
 * hook; sizing is a composed atom — `basis("220px")` for
 * a fixed track, `fluid` for the one that fills the
 * remaining row.
 */
export const column = <Msg>(
  parts: Parts,
  children: ReadonlyArray<Html<Msg>>,
): Html<Msg> =>
  el(
    "div",
    [style_("pm-col", flexCol, ...parts)],
    children,
  );

/**
 * A landmark content region inside a column: the
 * accessibility skeleton of the pattern. `pane(role)`
 * renders the role's semantic element (`nav`, `main`,
 * `aside` — see {@link landmarkTag}) around arbitrary
 * consumer content, with the `pm-pane` hook. Scroll,
 * padding, and measure caps are composed parts.
 */
export const pane =
  (role: PaneRole) =>
  <Msg>(
    parts: Parts,
    children: ReadonlyArray<Html<Msg>>,
  ): Html<Msg> =>
    el(
      landmarkTag(role),
      [style_("pm-pane", ...parts)],
      children,
    );

/** The `navigation` pane — renders a `<nav>` landmark. */
export const navPane = pane("navigation");
/** The `main` pane — renders the `<main>` landmark. */
export const mainPane = pane("main");
/** The `complementary` pane — renders an `<aside>`. */
export const asidePane = pane("complementary");
