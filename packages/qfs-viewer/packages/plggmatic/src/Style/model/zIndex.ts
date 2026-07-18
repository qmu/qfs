/**
 * The z-index stacking bands. A closed, semantically
 * named set — spaced (1 / 30 / 40 / 50) so a new layer
 * can slot between two without renumbering — that
 * replaces the ad-hoc integers which otherwise accumulate
 * across the shell. Exactly the oracle's stack plus the
 * example's sticky header:
 *
 * - `content` (1) — a sticky in-pane header inside a
 *   scrolling column (the example's `.ex-colhead`).
 * - `chrome` (30) — the sticky mobile bar.
 * - `backdrop` (40) — the dimmed scrim behind an open
 *   drawer.
 * - `overlay` (50) — the off-canvas drawer itself.
 *
 * The `zIndex` atom (Style/usecase/utilities) resolves a
 * band for inline styles; the value function feeds raw
 * CSS strings (media blocks) where an atom cannot reach.
 */
export type ZBand =
  "content" | "chrome" | "backdrop" | "overlay";

export const zBands: ReadonlyArray<ZBand> = [
  "content",
  "chrome",
  "backdrop",
  "overlay",
];

// The band→integer table. Exposed as `zBandTable` so a
// `Theme` can carry it (`defaultTheme.zBands` is exactly
// this); `zValue` reads it for the atom + the chrome media
// blocks.
export const zBandTable: Record<ZBand, number> = {
  content: 1,
  chrome: 30,
  backdrop: 40,
  overlay: 50,
};

/** A band's concrete stacking integer. */
export const zValue = (b: ZBand): number =>
  zBandTable[b];
