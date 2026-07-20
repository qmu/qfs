import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  button,
  declare,
  schedule,
  frameworkName,
  themeToggle,
  themeToggleClass,
  themeToggleCss,
  staticThemeToggle,
} from "plggmatic/index";
import {
  schemeCss,
  bg,
  colorVar,
  metricVar,
  syntaxKinds,
} from "plggmatic/styleEntry";

/**
 * plggmatic owns its UI engine (absorbed back from the
 * retired `plgg-ui` package). These assertions guard the
 * historical surface across that move: the runtime
 * surface and the `themeToggle*` family are reachable
 * from `plggmatic`, and the theme surface from
 * `plggmatic/style`. A broken export path (renamed engine
 * symbol, dropped subpath) fails here rather than in a
 * downstream consumer.
 */
test("plggmatic exports the engine runtime surface", () => {
  return all([
    check(button !== undefined, toBe(true)),
    check(declare !== undefined, toBe(true)),
    check(schedule !== undefined, toBe(true)),
    check(frameworkName, toBe("plggmatic")),
  ]);
});

test("plggmatic keeps the themeToggle* family on its root surface", () => {
  return all([
    check(themeToggle !== undefined, toBe(true)),
    check(
      themeToggleClass !== undefined,
      toBe(true),
    ),
    check(
      themeToggleCss !== undefined,
      toBe(true),
    ),
    check(
      staticThemeToggle !== undefined,
      toBe(true),
    ),
  ]);
});

test("plggmatic/style exports the engine theme surface", () => {
  return all([
    check(schemeCss !== undefined, toBe(true)),
    check(bg !== undefined, toBe(true)),
    check(colorVar !== undefined, toBe(true)),
    check(metricVar !== undefined, toBe(true)),
    check(syntaxKinds !== undefined, toBe(true)),
  ]);
});
