import { type SoftStr } from "plgg";
import {
  type Metric,
  metrics,
} from "plggmatic/Style/model/metric";
import { type Theme } from "plggmatic/Style/model/theme";

/**
 * The shell-metric custom properties as a single
 * scheme-INDEPENDENT `:root` block
 * (`--pm-shell-max:1440px;…`), in {@link metrics} order.
 * Mirrors `schemeCss`'s single-source contract, but
 * without a `html.dark` override — geometry does not
 * change by light/dark. Escape-safe (no `<`, `>`, `&`) so
 * it survives an SSR text escaper byte-for-byte; inject
 * ahead of the collected atomic rules so every
 * `var(--<prefix>-*)` metric reference resolves. The
 * lengths and namespace come from the supplied
 * {@link Theme}.
 */
export const metricCss = (
  theme: Theme,
): SoftStr =>
  `:root{${metrics
    .map(
      (m: Metric) =>
        `--${theme.prefix}-${m}:${theme.metrics[m]};`,
    )
    .join("")}}`;
