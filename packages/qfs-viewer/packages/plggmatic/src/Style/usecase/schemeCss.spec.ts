import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  schemeCss as schemeCssFor,
  schemeCssOf,
} from "plggmatic/Style/usecase/schemeCss";
import {
  asPalette,
  defaultPalette,
} from "plggmatic/Style/model/palette";
import { colors } from "plggmatic/Style/model/token";
import { schemes } from "plggmatic/Style/model/scheme";
import { defaultTheme } from "plggmatic/Style/model/theme";
import { isOk } from "plgg";

// The default theme reproduces the pre-parameterization
// scheme CSS byte-for-byte; `schemeCssOf(palette)` stays
// the palette-only override convenience.
const schemeCss = schemeCssFor(defaultTheme);

// The emitter maps over `colors`, so it emits every token
// for every scheme with no per-token logic. These specs
// pin that: the full matrix + neutral scale is present
// (including the decorative `--pm-border` the contrast
// spec leaves un-gated), and the block stays escape-safe so
// it survives an SSR text escaper byte-for-byte.

const count = (
  hay: string,
  needle: string,
): number => hay.split(needle).length - 1;

test("emits exactly schemes × colors custom-property declarations", () =>
  check(
    count(schemeCss, "--pm-"),
    toBe(schemes.length * colors.length),
  ));

test("emits the matrix and neutral tokens explicitly", () =>
  all([
    check(
      schemeCss.includes("--pm-primary-base:"),
      toBe(true),
    ),
    check(
      schemeCss.includes("--pm-info-surface:"),
      toBe(true),
    ),
    check(
      schemeCss.includes("--pm-border:"),
      toBe(true),
    ),
  ]));

test("scheme CSS is escape-safe (no <, >, &)", () =>
  all([
    check(schemeCss.includes("<"), toBe(false)),
    check(schemeCss.includes(">"), toBe(false)),
    check(schemeCss.includes("&"), toBe(false)),
  ]));

test("default schemeCss equals schemeCssOf(defaultPalette)", () =>
  check(
    schemeCss,
    toBe(schemeCssOf(defaultPalette)),
  ));

test("an override palette reaches every emitted declaration", () => {
  // A palette that is #000000 everywhere in both schemes.
  const rows = Object.fromEntries(
    colors.map((c) => [c, "#000000"]),
  );
  const overridden = asPalette({
    light: rows,
    dark: rows,
  });
  const css = isOk(overridden)
    ? schemeCssOf(overridden.content)
    : "";
  return all([
    check(isOk(overridden), toBe(true)),
    // every --pm-* declaration carries the override value…
    check(
      count(css, ":#000000;"),
      toBe(schemes.length * colors.length),
    ),
    // …and none carries a default value.
    check(css.includes("#ffffff"), toBe(false)),
  ]);
});
