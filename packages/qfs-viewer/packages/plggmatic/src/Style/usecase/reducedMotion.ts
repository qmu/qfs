import { type SoftStr } from "plgg";

/**
 * The framework-owned reduced-motion block, generalized
 * from the oracle's `prefers-reduced-motion:reduce` rule
 * (`baseCss.ts` lines 80–85): smooth scrolling drops back
 * to `auto` on the page and on the independently
 * scrolling `main` column.
 *
 * **Recorded rule:** any motion plggmatic ships — today
 * the `hoverDim`/`pressDim` feedback and the example's
 * drawer/snap transitions, tomorrow whatever the D9
 * scheduler animates — must be authored so THIS block
 * disables it (a transition gated behind the same query,
 * an animation reset to `none`). Motion is opt-out by
 * default (WCAG 2.3.3). Ticket 07 composes this block into
 * the ported theme rather than re-authoring it. Escape-safe
 * (no `<`, `>`, `&`).
 */
export const reducedMotionCss: SoftStr = `@media (prefers-reduced-motion:reduce){html{scroll-behavior:auto}main{scroll-behavior:auto}}`;
