/**
 * The display mode a consuming app holds BESIDE the
 * scheduled model (never inside it, never in the
 * declaration, never in the URL — D10). A closed union so
 * the render dispatcher's `match` is exhaustive: adding a
 * third mode is a `tsc` error at the dispatcher, not a
 * blank screen. Pure data — no persistence, no DOM.
 */
export type Mode = "multiColumn" | "singleColumn";

export const modes: ReadonlyArray<Mode> = [
  "multiColumn",
  "singleColumn",
];

/** Flips the mode — the runtime toggle a consumer wires. */
export const toggleMode = (m: Mode): Mode =>
  m === "multiColumn"
    ? "singleColumn"
    : "multiColumn";
