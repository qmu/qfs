import { type SoftStr } from "plgg";
import {
  type Html,
  type Flow,
  type Phrasing,
  type Attribute,
  h1,
  h2,
  h3,
  h4,
  div,
  text,
} from "plgg-view";
import {
  type TypeRole,
  style_,
  textColor,
  typeStyle,
  measure,
} from "plggmatic/styleEntry";

/**
 * A heading level. A closed union so a level maps to a
 * real `h1`–`h4` element (and its font token) — an
 * invalid level cannot be requested.
 */
export type HeadingLevel = 1 | 2 | 3 | 4;

// Each level's semantic element builder. A closed switch
// (no `default`) keeps it exhaustive: a new level is a
// compile error until it has an element.
const levelEl = (
  level: HeadingLevel,
): (<Msg>(
  attributes: ReadonlyArray<Attribute<Msg>>,
  children: ReadonlyArray<Phrasing<Msg>>,
) => Html<Msg>) => {
  switch (level) {
    case 1:
      return h1;
    case 2:
      return h2;
    case 3:
      return h3;
    case 4:
      return h4;
  }
};

// The prose type role for each heading level. Exhaustive
// over the level union — a level maps to its `h1`–`h4`
// entry in the shared {@link typeStyle} scale, so element,
// size, leading, and weight all come from one token.
const LEVEL_ROLE: Record<HeadingLevel, TypeRole> =
  {
    1: "h1",
    2: "h2",
    3: "h3",
    4: "h4",
  };

/**
 * The heading component. **Recorded rule**: the heading
 * `level` is a semantic prop that maps 1:1 to a real
 * `h1`–`h4` element, and its size / leading / weight are
 * drawn from the prose {@link typeStyle} scale by that
 * same level — so the document outline and the type scale
 * never drift apart (no "looks like an h2 but is a div").
 * The shipped scale is the guide's: level 1 renders
 * `1.875rem` at weight 400, not the old generic
 * `2xl`/`600`. Themed `text` ink.
 */
export const heading = (
  level: HeadingLevel,
  content: SoftStr,
): Html<never> =>
  levelEl(level)(
    [
      style_(
        typeStyle(LEVEL_ROLE[level]),
        textColor("text"),
      ),
    ],
    [text(content)],
  );

/**
 * The prose component. **Recorded rule**: prose is a
 * typographic container that establishes the reading
 * baseline once — themed body ink, the `body` type role
 * (`1rem` at line-height `1.75`), and the capped readable
 * `measure` (the `48rem` shell metric, via its custom
 * property) — for arbitrary flow content; per-element
 * prose rules (link underline weight, code badges, list
 * rhythm) are added one at a time as real documents demand
 * them, not pre-built here (emergent design system).
 */
export const prose = <Msg>(
  body: ReadonlyArray<Flow<Msg>>,
): Html<Msg, "div"> =>
  div(
    [
      style_(
        textColor("text"),
        typeStyle("body"),
        measure,
      ),
    ],
    body,
  );
