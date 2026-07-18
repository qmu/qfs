import { type SoftStr } from "plgg";
import { type Scheme } from "plggmatic/Style/model/scheme";
import { appearanceStorageKey } from "plggmatic/Style/model/appearance";

/**
 * The dependency-free, no-FOUC inline script that sets the
 * `dark` class BEFORE first paint, mirroring plggpress's
 * `HEAD_SCRIPT` in effect: read {@link appearanceStorageKey}
 * else `matchMedia('(prefers-color-scheme: dark)')`, add
 * `dark` to `document.documentElement.classList`, all in a
 * `try/catch` (private mode / blocked storage schemes from
 * the OS preference). Contains no `</script` inner sequence,
 * and is injected AFTER the SSR text escaper (see
 * {@link injectAppearanceScript}) so its `<` characters are
 * intentional markup, not escaped content.
 */
export const appearanceInitScript: SoftStr = `try{var k=localStorage.getItem(${JSON.stringify(appearanceStorageKey)});var d=k==="dark"||(k!=="light"&&window.matchMedia&&window.matchMedia("(prefers-color-scheme: dark)").matches);if(d){document.documentElement.classList.add("dark");}}catch(e){}`;

/**
 * Insert the init script just before `</head>`. A page with
 * no `</head>` passes through unchanged (idempotent guard,
 * per the plggpress precedent). Injected after escaping, so
 * the `<script>` tags are literal.
 */
export const injectAppearanceScript = (
  html: SoftStr,
): SoftStr =>
  html.includes("</head>")
    ? html.replace(
        "</head>",
        `<script>${appearanceInitScript}</script></head>`,
      )
    : html;

// The minimal document root the toggle helper needs — a
// `classList` that can add/remove `dark`. Kept structural so
// specs drive it with an in-memory fake (no DOM env, no `as`).
export type SchemeClassList = Readonly<{
  add: (token: string) => void;
  remove: (token: string) => void;
}>;
export type SchemeRoot = Readonly<{
  classList: SchemeClassList;
}>;

// The minimal storage the toggle helper writes to.
export type SchemeStorage = Readonly<{
  setItem: (key: string, value: string) => void;
}>;

/**
 * Apply and persist a scheme at the app's effect seam: set
 * or remove the `dark` class on the root, and store the
 * choice under {@link appearanceStorageKey}. Storage
 * failures (private mode) are swallowed — the class still
 * flips, and the OS preference still schemes on reload.
 * (When plgg-view gains `Cmd`/`Sub` in ticket 06 this wraps
 * as a `Cmd`; the adapter is intentionally thin.)
 */
export const applyScheme = (
  scheme: Scheme,
  root: SchemeRoot,
  storage: SchemeStorage,
): void => {
  if (scheme === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
  try {
    storage.setItem(appearanceStorageKey, scheme);
  } catch {
    // private mode / blocked storage — non-fatal
  }
};
