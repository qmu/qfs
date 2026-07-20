/**
 * The plggmatic Render module: the screen-mode renderers
 * (D10) — pure projections of ticket 09's scheduled
 * `Scene` into a display. The multi-column mode (ticket
 * 10) and the single-column mode + runtime `Mode`
 * dispatcher (ticket 11) live here; each is
 * `(Scene) => Html<SchedulerMsg>`, stores no state, and
 * touches no `window`. `renderMode(mode)(scene)` is the
 * public entry; the two renderers are exported for direct
 * use and testing.
 */
export {
  type Mode,
  modes,
  toggleMode,
} from "plggmatic/Render/model/mode";
export {
  type Screen,
  currentScreen,
} from "plggmatic/Render/model/screen";
export {
  type HeaderLink,
  type ExtraColumn,
  type MultiColumnOptions,
  multiColumn,
  multiColumnWith,
  crumbsOf,
} from "plggmatic/Render/usecase/multiColumn";
export { singleColumn } from "plggmatic/Render/usecase/singleColumn";
export { renderMode } from "plggmatic/Render/usecase/renderMode";
