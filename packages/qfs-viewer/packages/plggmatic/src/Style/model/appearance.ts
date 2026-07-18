import {
  type Option,
  type SoftStr,
  matchOption,
} from "plgg";
import { type Scheme } from "plggmatic/Style/model/scheme";

/**
 * The localStorage key the visitor's scheme choice is
 * persisted under. Preserved verbatim per **D16**: the key
 * predates plggmatic (born in plggpress's theme,
 * `20260630`), so keeping it means existing visitors keep
 * their saved choice across the plggpress→plggmatic theme
 * cutover. It deliberately does NOT follow the `--pm-*`
 * rename — only the CSS variables were renamed, not the
 * storage contract.
 */
export const appearanceStorageKey =
  "vp-appearance";

/**
 * The pure scheme decision, kept DOM-free so the whole
 * table is spec-coverable without a browser: a stored
 * `"dark"`/`"light"` wins; anything else (absent, or an
 * unrecognized value) falls back to the OS
 * `prefers-color-scheme`. This is exactly plggpress's
 * `HEAD_SCRIPT` decision order.
 */
export const decideScheme = (
  stored: Option<SoftStr>,
  prefersDark: boolean,
): Scheme =>
  matchOption(
    () => (prefersDark ? "dark" : "light"),
    (value: SoftStr): Scheme =>
      value === "dark"
        ? "dark"
        : value === "light"
          ? "light"
          : prefersDark
            ? "dark"
            : "light",
  )(stored);
