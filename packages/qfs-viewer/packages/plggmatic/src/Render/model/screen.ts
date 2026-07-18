import { type Option, fromNullable } from "plgg";
import {
  type Scene,
  type Level,
} from "plggmatic/Schedule/model/Scene";

/**
 * The single-column mode shows ONE operation per screen —
 * and the current screen is DERIVED, never stored (the
 * strongest answer to ticket 09's tenet (g): this
 * renderer holds no state at all). The current screen is
 * the DEEPEST level of the scheduled `Scene` (the menu
 * when nothing is open, the deepest list while browsing,
 * the detail when an item is selected). A `Level` already
 * IS the screen union (menu / list / detail), so `Screen`
 * aliases it; the confirmation is a renderer-owned overlay
 * in both modes (parity is reachability, not identical
 * DOM). The back target for each screen is the level's own
 * truncating `back`, obtained from the scheduler — never
 * recomputed renderer-side.
 */
export type Screen = Level;

/**
 * The current screen — the deepest level. `None` only for
 * a structurally-empty scene (the scheduler always emits
 * at least the menu level, so in practice this is
 * `Some`); the renderer degrades to an empty screen rather
 * than crashing.
 */
export const currentScreen = (
  scene: Scene,
): Option<Screen> =>
  fromNullable(
    scene.levels[scene.levels.length - 1],
  );
