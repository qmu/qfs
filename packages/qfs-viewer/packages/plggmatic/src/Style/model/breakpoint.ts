import { type SoftStr } from "plgg";

/**
 * The responsive breakpoints plggmatic names. A closed
 * union: `sm` (the phone column — the oracle's
 * `max-width:639px` heading block), `snap` (the
 * multi-column example's horizontal-snap boundary,
 * `min-width:900px`), and `lg` (the docs app-shell
 * boundary, `min-width:1024px`).
 *
 * **Two shells, two boundaries — do NOT unify.** `lg`
 * gates the docs sidebar shell; `snap` gates the
 * example's column strip. They are different layouts with
 * different collapse physics; a merge only becomes
 * correct if the D10 scheduler makes one derivable from
 * the other.
 */
export type Breakpoint = "sm" | "snap" | "lg";

export const breakpoints: ReadonlyArray<Breakpoint> =
  ["sm", "snap", "lg"];

// Each breakpoint's boundary width in CSS pixels.
const WIDTH: Record<Breakpoint, number> = {
  sm: 640,
  snap: 900,
  lg: 1024,
};

/** A breakpoint's boundary width in CSS pixels. */
export const breakpointPx = (
  b: Breakpoint,
): number => WIDTH[b];

/**
 * A `min-width` media condition AT the breakpoint, e.g.
 * `(min-width:1024px)`. Consumed as
 * `@media ${minWidth("lg")}{…}`.
 *
 * **Breakpoints are TS constants, never `--pm-*` custom
 * properties.** A `@media` query cannot resolve `var()`,
 * so these boundaries are build-time values baked into
 * the CSS-emitting code — a future contributor "cleaning
 * up" breakpoints into custom properties would silently
 * break every media query. This is a hard constraint, not
 * a style choice.
 */
export const minWidth = (
  b: Breakpoint,
): SoftStr => `(min-width:${WIDTH[b]}px)`;

/**
 * A `max-width` media condition one pixel BELOW the
 * breakpoint (`sm` → `(max-width:639px)`) — the exact
 * complement of {@link minWidth}, so the pair never
 * overlaps on the boundary pixel. Matches the oracle and
 * the example verbatim (`639`/`899`/`1023`).
 */
export const maxWidth = (
  b: Breakpoint,
): SoftStr => `(max-width:${WIDTH[b] - 1}px)`;
