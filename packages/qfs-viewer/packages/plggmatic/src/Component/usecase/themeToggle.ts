import { type SoftStr } from "plgg";
import {
  type Html,
  button as buttonEl,
  svg,
  path,
  attr,
  class_,
  type_,
  onClick,
} from "plgg-view";
// Atoms imported from the deep modules, NOT the
// `styleEntry` barrel. themeToggle is routed onto the
// `/style` surface through `styleEntry`, so a barrel
// import here would close the cycle
// styleEntry → themeToggle → styleEntry and leave
// `colorVar` / the color atoms uninitialized when
// `themeToggleCss` computes at module load. The
// scheme-aware `bg`/`textColor`/`border`/`colorVar` come
// from `Style` (shadowing plgg-view's literals);
// `style_`/`rounded`/`p`/`pointer` are plgg-view's — the
// same symbols the barrel re-exported.
import {
  type Scheme,
  bg,
  textColor,
  border,
  colorVar,
  defaultTheme,
} from "plggmatic/Style";
import {
  style_,
  rounded,
  p,
  pointer,
} from "plgg-view/style";
import {
  focusRing,
  hoverDim,
} from "plggmatic/Component/model/interaction";

// `themeToggleCss` bakes its chrome at module load, so it
// binds `colorVar` to the default theme (the SSG toggle
// ships the monochrome `--pm-*` namespace); a consumer's
// scheme still resolves the values at paint time.
const cvar = colorVar(defaultTheme);

// The oracle's sun (8-ray) and crescent-moon paths,
// single `currentColor` fills, ported from plggpress's
// navBar so the toggle inherits the control's ink and
// flips with the theme like text.
const SUN_D: SoftStr =
  "M12 18a6 6 0 1 1 0-12 6 6 0 0 1 0 12zM11 1h2v3h-2zm0 19h2v3h-2zM3.515 4.929l1.414-1.414L7.05 5.636 5.636 7.05 3.515 4.93zM16.95 18.364l1.414-1.414 2.121 2.121-1.414 1.414-2.121-2.121zm2.121-14.85l1.414 1.415-2.121 2.121-1.414-1.414 2.121-2.121zM5.636 16.95l1.414 1.414-2.121 2.121-1.414-1.414 2.121-2.121zM23 11v2h-3v-2zM4 11v2H1v-2z";
const MOON_D: SoftStr =
  "M9.822 2.238a9 9 0 0 0 11.94 11.94C20.768 18.654 16.775 22 12 22 6.477 22 2 17.523 2 12c0-4.775 3.346-8.768 7.822-9.762z";

const icon = (d: SoftStr): Html<never, "svg"> =>
  svg(
    [
      attr("viewBox", "0 0 24 24"),
      attr("width", "18"),
      attr("height", "18"),
      attr("fill", "currentColor"),
      attr("aria-hidden", "true"),
    ],
    [path([attr("d", d)], [])],
  );

/**
 * A theme toggle's props. `scheme` is the CURRENTLY
 * active scheme (so the button shows where a click will
 * take you); `toggle` is the `Msg` a click produces.
 */
export type ThemeToggleProps<Msg> = Readonly<{
  scheme: Scheme;
  toggle: Msg;
}>;

/**
 * The appearance-switch component, exercising the token
 * layer's scheme mechanism. **Recorded rule**: the
 * framework ships the VIEW ONLY — the toggle renders the
 * current scheme's icon (sun in light, moon in dark) and
 * emits a `toggle` `Msg`; applying the `dark` class to
 * `<html>` is the app's effect seam, wired through the
 * framework-owned appearance contract —
 * `applyScheme` / `appearanceStorageKey` (the preserved
 * `vp-appearance` key) / `appearanceInitScript` in
 * `Style/usecase/appearanceScript.ts` — so the component
 * stays pure. The `aria-label` names the destination
 * scheme, and the icon is a non-color cue (shape, not
 * hue) for the current one. Carries the shared focus
 * ring and hover feedback.
 */
export const themeToggle = <Msg>(
  props: ThemeToggleProps<Msg>,
): Html<Msg, "button"> =>
  buttonEl(
    [
      type_("button"),
      attr(
        "aria-label",
        props.scheme === "light"
          ? "Switch to dark mode"
          : "Switch to light mode",
      ),
      onClick(props.toggle),
      style_(
        bg("surface"),
        textColor("text"),
        border,
        rounded("full"),
        p(2),
        pointer,
        focusRing,
        hoverDim,
      ),
    ],
    [
      props.scheme === "light"
        ? icon(SUN_D)
        : icon(MOON_D),
    ],
  );

/**
 * The stable class hook the SSG toggle carries. A
 * static-site host binds every rendered toggle by this
 * class from its no-runtime body script, and the
 * icon-switch CSS in {@link themeToggleCss} targets it —
 * so the literal is never re-typed at the consumer.
 */
export const themeToggleClass: SoftStr =
  "pm-theme-toggle";

// The icon classes the scheme-switch CSS keys on. Both
// icons are ALWAYS rendered (SSG can't know the visitor's
// scheme at build time); CSS shows one per `html.dark`.
const SUN_CLASS: SoftStr = "pm-sun";
const MOON_CLASS: SoftStr = "pm-moon";

// A class-tagged static icon (no `style_` atoms — the
// SSG toggle is fully class-driven so a host with no
// plgg-view runtime can still style and wire it).
const staticIcon = (
  cls: SoftStr,
  d: SoftStr,
): Html<never, "svg"> =>
  svg(
    [
      class_(cls),
      attr("viewBox", "0 0 24 24"),
      attr("fill", "currentColor"),
      attr("aria-hidden", "true"),
    ],
    [path([attr("d", d)], [])],
  );

/**
 * The SSG-capable appearance toggle: a static
 * `Html<never, "button">` that renders BOTH icons (the
 * active one chosen by CSS on `html.dark`, NOT by a
 * build-time scheme) and carries {@link themeToggleClass}
 * so a host's no-runtime body script can wire the click.
 *
 * The runtime {@link themeToggle} is for TEA apps that
 * re-render on a scheme `Msg`; a static-site host (no
 * plgg-view runtime on the page, `renderToString` drops
 * every handler) can neither dispatch a `Msg` nor know the
 * visitor's scheme when the page is built, so it renders
 * this instead and drives it through the framework-owned
 * appearance contract (`appearanceStorageKey` /
 * `applyScheme` / `appearanceInitScript`). Chrome and
 * icon-switch live in {@link themeToggleCss}; escape-safe.
 */
export const staticThemeToggle: Html<
  never,
  "button"
> = buttonEl(
  [
    type_("button"),
    attr("aria-label", "Toggle dark mode"),
    class_(themeToggleClass),
  ],
  [
    staticIcon(SUN_CLASS, SUN_D),
    staticIcon(MOON_CLASS, MOON_D),
  ],
);

/**
 * The SSG toggle's chrome + icon-switch as one escape-safe
 * CSS body (no `<`, `>`, `&`; descendant selectors only, no
 * child combinators) so it survives a host's SSR text
 * escaper byte-for-byte. Token-driven — a circular,
 * bordered control on the neutral `surface`, the sun shown
 * in light and the crescent under `html.dark` (a shape
 * cue, not a hue), with the shared 2px focus ring and the
 * `primary` hover edge matching the runtime
 * {@link themeToggle}. A host composes it into its document
 * `<style>` AFTER the scheme variables (it references
 * `var(--pm-*)`) and ahead of the collected atomic rules.
 */
export const themeToggleCss: SoftStr =
  `.${themeToggleClass}{display:inline-flex;` +
  `align-items:center;justify-content:center;` +
  `width:38px;height:38px;border-radius:50%;` +
  `border:1px solid ${cvar("border")};` +
  `background:${cvar("surface")};` +
  `color:${cvar("text")};padding:0;` +
  `cursor:pointer;transition:background-color ` +
  `0.15s,border-color 0.15s}` +
  `.${themeToggleClass}:hover{border-color:` +
  `${cvar("primary-base")}}` +
  `.${themeToggleClass}:focus-visible{outline:2px ` +
  `solid ${cvar("primary-base")};` +
  `outline-offset:2px}` +
  `.${SUN_CLASS},.${MOON_CLASS}{width:18px;` +
  `height:18px;display:block}` +
  `.${themeToggleClass} .${MOON_CLASS}{display:none}` +
  `html.dark .${themeToggleClass} .${SUN_CLASS}` +
  `{display:none}` +
  `html.dark .${themeToggleClass} .${MOON_CLASS}` +
  `{display:block}`;
