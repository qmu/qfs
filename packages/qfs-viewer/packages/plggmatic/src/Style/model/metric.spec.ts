import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type Metric,
  metrics,
  metricValue,
  metricVar as metricVarFor,
} from "plggmatic/Style/model/metric";
import { defaultTheme } from "plggmatic/Style/model/theme";

// `metricVar(theme)(m)` under the default theme is exactly
// the old `metricVar(m)` — `var(--pm-<metric>)`.
const metricVar = metricVarFor(defaultTheme);

const SEEN: Record<Metric, true> = {
  "shell-max": true,
  sidebar: true,
  measure: true,
  rail: true,
};

test("metrics lists its union once", () =>
  all([
    check(
      metrics.every((m) => SEEN[m]),
      toBe(true),
    ),
    check(
      metrics.length,
      toBe(new Set(metrics).size),
    ),
    check(metrics.length, toBe(4)),
  ]));

test("metric values match the oracle geometry", () =>
  all([
    check(
      metricValue("shell-max"),
      toBe("1440px"),
    ),
    check(metricValue("sidebar"), toBe("256px")),
    check(metricValue("measure"), toBe("48rem")),
    check(metricValue("rail"), toBe("48px")),
  ]));

test("metricVar references the --pm namespace", () =>
  all([
    check(
      metricVar("measure"),
      toBe("var(--pm-measure)"),
    ),
    check(
      metricVar("rail"),
      toBe("var(--pm-rail)"),
    ),
  ]));
