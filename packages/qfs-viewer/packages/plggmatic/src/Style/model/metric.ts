import { type SoftStr } from "plgg";
// Type-only (erased): `Theme` and this module reference
// each other's types; `defaultTheme` reads `metricTable`
// at runtime, so only this direction is a value edge.
import { type Theme } from "plggmatic/Style/model/theme";

/**
 * The shell-geometry metrics — the fixed dimensions of
 * the app shell, lifted value-for-value from the qmu
 * oracle (`--vp-shell-max` / `--vp-sidebar-w` /
 * `--vp-rail-w` and the `.vp-doc` prose measure). A
 * closed union so a missing dimension is a `tsc` error.
 *
 * Unlike {@link Breakpoint}s these ARE emitted as `--pm-*`
 * custom properties: they are `var()`-consumable (used in
 * ordinary declarations, never inside `@media`), so the
 * shell CSS and the `prose` measure reference them through
 * {@link metricVar} and one edit retunes the geometry
 * everywhere. Scheme-independent — geometry does not
 * change by light/dark, so there is no `html.dark`
 * override (see the metric CSS emitter).
 */
export type Metric =
  "shell-max" | "sidebar" | "measure" | "rail";

export const metrics: ReadonlyArray<Metric> = [
  "shell-max",
  "sidebar",
  "measure",
  "rail",
];

// Each metric's concrete length. `shell-max` 1440px (the
// centred shell cap), `sidebar` 256px (the nav column),
// `measure` 48rem (the prose reading measure), `rail`
// 48px (the chrome-rail thickness, reused as the
// example's top-bar height). Exposed as `metricTable` so a
// `Theme` can carry it (and an override can supply its
// own); `defaultTheme.metrics` is exactly this.
export const metricTable: Record<
  Metric,
  SoftStr
> = {
  "shell-max": "1440px",
  sidebar: "256px",
  measure: "48rem",
  rail: "48px",
};

/** A metric's concrete length (build-time value). */
export const metricValue = (m: Metric): SoftStr =>
  metricTable[m];

/**
 * The `var(--<prefix>-<metric>)` reference for a metric —
 * the runtime handle the shell CSS and the `measure` atom
 * emit, mirroring `colorVar`. Resolved by the metrics
 * `:root` block (see the metric CSS emitter). The prefix
 * comes from the supplied {@link Theme} (`pm` by default),
 * so a consumer that renames its custom-property namespace
 * flows through here. Curried `metricVar(theme)(m)` so a
 * theme-bound emitter fixes the namespace once.
 */
export const metricVar =
  (theme: Theme) =>
  (m: Metric): SoftStr =>
    `var(--${theme.prefix}-${m})`;
