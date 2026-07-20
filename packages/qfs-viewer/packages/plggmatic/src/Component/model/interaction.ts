// Atoms imported from the deep modules, NOT the
// `styleEntry` barrel. themeToggle is routed onto the
// `/style` surface through `styleEntry`, so a barrel
// import here would close the cycle
// styleEntry → themeToggle → interaction → styleEntry
// and leave these atoms uninitialized at module load.
// `outline` is the scheme-aware version from `Style`
// (shadowing plgg-view's); `decl`/`Variant` are
// plgg-view's — the same symbols the barrel re-exported.
import { outline } from "plggmatic/Style";
import {
  type Variant,
  decl,
} from "plgg-view/style";

/**
 * The standardized interaction states every plggmatic
 * component speaks — defined ONCE here and imported by
 * all, so a button, a link, and a toggle give identical
 * feedback for the same gesture (interaction-design
 * standard). A closed union; a component that invents a
 * new state must add it here, with a rule, not inline.
 */
export type InteractionState =
  | "default"
  | "hover"
  | "focus"
  | "active"
  | "disabled";

/**
 * THE recorded interaction rule of the framework, shared
 * by every focusable component: a keyboard focus ring
 * that is a real 2px outline offset from the control —
 * a NON-COLOR affordance (geometry, not just a hue), so
 * focus is legible independent of color vision
 * (accessibility-first). Scoped to `:focus-visible` so
 * it appears for keyboard/AT focus but not on mouse
 * press. The ring color is the themed `primary` role, so
 * it reschemes with the UI.
 */
export const focusRing: Variant = {
  selector: ":focus-visible",
  styles: [
    ...outline("primary-base"),
    ...decl("outline-offset", "2px"),
  ],
};

/**
 * Shared hover feedback: a slight dim. Kept as opacity
 * (not a new hover color token) so every component dims
 * consistently without expanding the palette before a
 * component needs a dedicated hover role.
 *
 * Scope note (D9 + ticket 05): this opacity-dim rule is
 * the COMPONENT hover feedback — a control dimming under
 * the pointer. It is NOT the qmu inverted-pill THEME idiom
 * (a link/leaf swapping to `primary-base` fill with a
 * neutral-`surface` label), which is expressed through the
 * existing color tokens and recorded in
 * `Style/model/token.ts`'s hover decision, not here. Both
 * rules stand; neither adds a hover color token.
 */
export const hoverDim: Variant = {
  selector: ":hover",
  styles: decl("opacity", "0.9"),
};

/** Shared press feedback: a deeper dim on `:active`. */
export const pressDim: Variant = {
  selector: ":active",
  styles: decl("opacity", "0.8"),
};
