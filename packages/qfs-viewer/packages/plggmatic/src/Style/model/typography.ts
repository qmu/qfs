import {
  type SoftStr,
  type Option,
  some,
  none,
} from "plgg";

/**
 * The prose type roles plggmatic tokenizes — the guide's
 * document scale, NOT plgg-view's generic `FontSize`
 * ladder (which tops out at `2xl = 1.5rem`, below the
 * guide `h1` of `1.875rem`, so it cannot reach the scale
 * at all). A closed union so a role with no entry — or a
 * typo like `typeScale["h5"]` — is a `tsc` error; ticket
 * 07 emits the ported theme's heading CSS from this one
 * map, base scale AND phone-column block.
 *
 * Sourced value-for-value from the qmu.co.jp oracle port
 * carried in `plggpress/src/theme/baseCss.ts` (the
 * `.vp-doc h1`–`h4` block and the `body.vp` base): a calm
 * ~1.25 modular scale on a 1rem body, every heading at
 * weight 400 (regular), no letter-spacing.
 */
export type TypeRole =
  "h1" | "h2" | "h3" | "h4" | "body";

export const typeRoles: ReadonlyArray<TypeRole> =
  ["h1", "h2", "h3", "h4", "body"];

/**
 * The closed font-weight set — the ONLY three weights the
 * oracle uses: `regular` body & headings, `medium` links,
 * wordmark, active nav and buttons, `semibold` section /
 * group titles and table headers. A union, not a free
 * `number`, so `weight(700)` is a compile error until a
 * component earns the weight (the emergent doctrine, one
 * tier down from the color roles).
 */
export type FontWeight = 400 | 500 | 600;

export const fontWeights: ReadonlyArray<FontWeight> =
  [400, 500, 600];

/** Regular — body copy and every heading (weight 400). */
export const regular: FontWeight = 400;
/** Medium — links, wordmark, active nav, buttons. */
export const medium: FontWeight = 500;
/** Semibold — section / group titles, table headers. */
export const semibold: FontWeight = 600;

/**
 * A heading role's compact (sub-`sm`) override: the
 * phone-column `size`, and — only where the oracle sets
 * one — a `lineHeight`. `h3` narrows its size but keeps
 * its base leading, so its `lineHeight` is `None` (absence
 * is `Option`, never a sentinel). Recorded so ticket 07
 * emits the oracle's `max-width:639px` media block from
 * the same source as the base scale.
 */
export type CompactType = Readonly<{
  size: SoftStr;
  lineHeight: Option<SoftStr>;
}>;

/**
 * One prose role's metrics: the base `size`, its
 * (unitless, inheritance-safe) `lineHeight`, its
 * `weight`, and the optional `compact` phone-column
 * override. `h4` and `body` have no compact variant
 * (`None`).
 */
export type TypeScale = Readonly<{
  size: SoftStr;
  lineHeight: SoftStr;
  weight: FontWeight;
  compact: Option<CompactType>;
}>;

/**
 * Every {@link TypeRole}'s metrics, from the oracle:
 * `h1 1.875rem/1.25`, `h2 1.5rem/1.3`,
 * `h3 1.1875rem/1.45`, `h4 1.0625rem/1.5`, all weight
 * 400; `body 1rem/1.75/400`. The compact sizes
 * (`h1 1.75rem`, `h2 1.375rem`, `h3 1.125rem`) are the
 * oracle's `max-width:639px` block — h1/h2 re-state their
 * (unchanged) leading, h3 keeps its base leading. The
 * `Record` makes it exhaustive: a missing role is a `tsc`
 * error.
 */
export const typeScale: Record<
  TypeRole,
  TypeScale
> = {
  h1: {
    size: "1.875rem",
    lineHeight: "1.25",
    weight: 400,
    compact: some({
      size: "1.75rem",
      lineHeight: some("1.25"),
    }),
  },
  h2: {
    size: "1.5rem",
    lineHeight: "1.3",
    weight: 400,
    compact: some({
      size: "1.375rem",
      lineHeight: some("1.3"),
    }),
  },
  h3: {
    size: "1.1875rem",
    lineHeight: "1.45",
    weight: 400,
    compact: some({
      size: "1.125rem",
      lineHeight: none(),
    }),
  },
  h4: {
    size: "1.0625rem",
    lineHeight: "1.5",
    weight: 400,
    compact: none(),
  },
  body: {
    size: "1rem",
    lineHeight: "1.75",
    weight: 400,
    compact: none(),
  },
};

/**
 * The sans font stack — the oracle's `body.vp`
 * `font-family` verbatim (`"Inter"` first, then the
 * system-UI fallbacks and the emoji families). A recorded
 * token; ticket 07 applies it as the ported theme's base
 * family. Escape-safe (no `<`, `>`, `&`).
 */
export const sansFontStack: SoftStr = `"Inter",ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif,"Apple Color Emoji","Segoe UI Emoji"`;
