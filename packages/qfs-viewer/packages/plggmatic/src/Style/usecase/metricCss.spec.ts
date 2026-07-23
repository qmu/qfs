import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { metricCss as metricCssFor } from "plggmatic/Style/usecase/metricCss";
import {
  metrics,
  metricValue,
} from "plggmatic/Style/model/metric";
import { defaultTheme } from "plggmatic/Style/model/theme";

// The default theme reproduces the pre-parameterization
// `--pm-*` output byte-for-byte; asserting on it proves
// `metricCss(theme)` unchanged the emitted CSS.
const metricCss = metricCssFor(defaultTheme);

const count = (
  hay: string,
  needle: string,
): number => hay.split(needle).length - 1;

test("emits one --pm-* declaration per metric, at :root", () =>
  all([
    check(
      metricCss.startsWith(":root{"),
      toBe(true),
    ),
    check(metricCss.endsWith("}"), toBe(true)),
    check(
      count(metricCss, "--pm-"),
      toBe(metrics.length),
    ),
  ]));

test("carries each metric's value", () =>
  all(
    metrics.map((m) =>
      check(
        metricCss.includes(
          `--pm-${m}:${metricValue(m)};`,
        ),
        toBe(true),
      ),
    ),
  ));

test("metric CSS is escape-safe (no <, >, &)", () =>
  all([
    check(metricCss.includes("<"), toBe(false)),
    check(metricCss.includes(">"), toBe(false)),
    check(metricCss.includes("&"), toBe(false)),
  ]));

// Geometry is scheme-independent — unlike schemeCss there
// is no html.dark override.
test("no html.dark override", () =>
  check(
    metricCss.includes("html.dark"),
    toBe(false),
  ));
