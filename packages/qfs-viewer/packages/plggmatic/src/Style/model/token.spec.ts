import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type SemanticRole,
  type Variant,
  type Neutral,
  colors,
  semanticRoles,
  variants,
  neutrals,
  colorVar as colorVarFor,
} from "plggmatic/Style/model/token";
import {
  colorHex,
  hex,
} from "plggmatic/Style/model/palette";
import { schemes } from "plggmatic/Style/model/scheme";
import { defaultTheme } from "plggmatic/Style/model/theme";

// `colorVar(theme)(c)` under the default theme is exactly
// the old `colorVar(c)` — `var(--pm-<token>)`.
const colorVar = colorVarFor(defaultTheme);

// A #rrggbb literal — the shape every palette value must
// take so the scheme emitter produces valid CSS. `colorHex`
// now returns a branded HexColor (palette.ts), so its raw
// string is always well-formed; this pins that.
const isHex = (v: string): boolean =>
  /^#[0-9a-f]{6}$/.test(v);

test("every color has a valid hex in every scheme", () =>
  all(
    schemes.flatMap((scheme) =>
      colors.map((c) =>
        check(
          isHex(hex(colorHex(scheme, c))),
          toBe(true),
        ),
      ),
    ),
  ));

// Compile-time exhaustiveness pins on the SOURCE unions:
// a member added to a union but missing from its array
// (or vice versa) fails `tsc` before any test runs.
const SEEN_ROLE: Record<SemanticRole, true> = {
  primary: true,
  success: true,
  danger: true,
  warning: true,
  info: true,
};
const SEEN_VARIANT: Record<Variant, true> = {
  base: true,
  text: true,
  surface: true,
  border: true,
};
const SEEN_NEUTRAL: Record<Neutral, true> = {
  surface: true,
  "surface-2": true,
  text: true,
  muted: true,
  border: true,
};

test("role/variant/neutral arrays list their unions once", () =>
  all([
    check(
      semanticRoles.every((r) => SEEN_ROLE[r]),
      toBe(true),
    ),
    check(
      variants.every((v) => SEEN_VARIANT[v]),
      toBe(true),
    ),
    check(
      neutrals.every((n) => SEEN_NEUTRAL[n]),
      toBe(true),
    ),
    check(
      semanticRoles.length,
      toBe(new Set(semanticRoles).size),
    ),
  ]));

test("colors is the derived 25-member matrix + neutrals, unique", () =>
  all([
    check(
      colors.length,
      toBe(
        semanticRoles.length * variants.length +
          neutrals.length,
      ),
    ),
    check(colors.length, toBe(25)),
    check(
      colors.length,
      toBe(new Set(colors).size),
    ),
  ]));

test("colorVar references the --pm namespace for matrix and neutral tokens", () =>
  all([
    check(
      colorVar("primary-base"),
      toBe("var(--pm-primary-base)"),
    ),
    check(
      colorVar("surface"),
      toBe("var(--pm-surface)"),
    ),
  ]));
