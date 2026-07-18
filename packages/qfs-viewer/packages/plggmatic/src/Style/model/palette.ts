import {
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
  isOk,
  isErr,
  box,
} from "plgg";
import {
  type Color,
  colors,
} from "plggmatic/Style/model/token";
import {
  type Scheme,
  schemes,
} from "plggmatic/Style/model/scheme";
import {
  type HexColor,
  asHexColor,
  hex,
} from "plggmatic/Style/model/hexColor";

/**
 * A full palette: every {@link Color} token, in every
 * {@link Scheme}, as a validated {@link HexColor}. The
 * `Record` shape makes it exhaustive twice over — a missing
 * scheme or token is a `tsc` error — so an override can
 * never ship a hole. Palette *data*, split from the token
 * *vocabulary* (`token.ts`) so an app can supply its own
 * brand colors without forking the framework.
 */
export type Palette = Record<
  Scheme,
  Record<Color, HexColor>
>;

// One scheme's row as a typed 25-key LITERAL (no `as`);
// parameterized by a getter so the same shape backs the
// default palette and a validated override.
const rowOf = (
  get: (c: Color) => HexColor,
): Record<Color, HexColor> => ({
  surface: get("surface"),
  "surface-2": get("surface-2"),
  text: get("text"),
  muted: get("muted"),
  border: get("border"),
  "primary-base": get("primary-base"),
  "primary-text": get("primary-text"),
  "primary-surface": get("primary-surface"),
  "primary-border": get("primary-border"),
  "success-base": get("success-base"),
  "success-text": get("success-text"),
  "success-surface": get("success-surface"),
  "success-border": get("success-border"),
  "danger-base": get("danger-base"),
  "danger-text": get("danger-text"),
  "danger-surface": get("danger-surface"),
  "danger-border": get("danger-border"),
  "warning-base": get("warning-base"),
  "warning-text": get("warning-text"),
  "warning-surface": get("warning-surface"),
  "warning-border": get("warning-border"),
  "info-base": get("info-base"),
  "info-text": get("info-text"),
  "info-surface": get("info-surface"),
  "info-border": get("info-border"),
});

const paletteOf = (
  get: (scheme: Scheme, c: Color) => HexColor,
): Palette => ({
  light: rowOf((c) => get("light", c)),
  dark: rowOf((c) => get("dark", c)),
});

// The monochrome default (D9): the qmu.co.jp oracle port.
// Raw literals live here; every one is validated through
// `asHexColor` when the palette is built.
const DEFAULT_RAW: Record<
  Scheme,
  Record<Color, string>
> = {
  light: {
    surface: "#ffffff",
    "surface-2": "#f6f6f7",
    text: "#1f1f22",
    muted: "#5b5b61",
    border: "#ededee",
    "primary-base": "#111111",
    "primary-text": "#111111",
    "primary-surface": "#f6f6f7",
    "primary-border": "#767679",
    "success-base": "#047857",
    "success-text": "#065f46",
    "success-surface": "#ecfdf5",
    "success-border": "#059669",
    "danger-base": "#b91c1c",
    "danger-text": "#991b1b",
    "danger-surface": "#fef2f2",
    "danger-border": "#dc2626",
    "warning-base": "#92400e",
    "warning-text": "#78350f",
    "warning-surface": "#fffbeb",
    "warning-border": "#b45309",
    "info-base": "#1d4ed8",
    "info-text": "#1e40af",
    "info-surface": "#eff6ff",
    "info-border": "#2563eb",
  },
  dark: {
    surface: "#1b1b1f",
    "surface-2": "#202127",
    text: "#dfdfe4",
    muted: "#8d8d95",
    border: "#262629",
    "primary-base": "#f4f4f4",
    "primary-text": "#f4f4f4",
    "primary-surface": "#202127",
    "primary-border": "#8a8a90",
    "success-base": "#34d399",
    "success-text": "#34d399",
    "success-surface": "#022c22",
    "success-border": "#34d399",
    "danger-base": "#f87171",
    "danger-text": "#f87171",
    "danger-surface": "#450a0a",
    "danger-border": "#f87171",
    "warning-base": "#fbbf24",
    "warning-text": "#fbbf24",
    "warning-surface": "#451a03",
    "warning-border": "#fbbf24",
    "info-base": "#60a5fa",
    "info-text": "#60a5fa",
    "info-surface": "#172554",
    "info-border": "#60a5fa",
  },
};

// Direct mint of the compile-time-known-good default
// literals — the public caster `asHexColor` guards UNTRUSTED
// input (config overrides); the framework's own default is
// trusted and validated separately by the `token.spec`
// isHex check over every `colorHex`, so this needs no dead
// Err branch. `box` is a plgg constructor, not an escape
// hatch.
const mint = (s: string): HexColor =>
  box("HexColor")(s);

export const defaultPalette: Palette = paletteOf(
  (scheme, c) => mint(DEFAULT_RAW[scheme][c]),
);

/**
 * A token's {@link HexColor} in a palette / scheme.
 */
export const paletteHex = (
  palette: Palette,
  scheme: Scheme,
  c: Color,
): HexColor => palette[scheme][c];

/**
 * A token's {@link HexColor} in the DEFAULT palette — the
 * zero-config path the emitter and the contrast gate use.
 * (Was `token.ts`'s `colorHex`; now returns the branded
 * color — `hex()` unwraps the raw string.)
 */
export const colorHex = (
  scheme: Scheme,
  c: Color,
): HexColor => defaultPalette[scheme][c];

const at = (
  obj: unknown,
  key: string,
): unknown =>
  typeof obj === "object" &&
  obj !== null &&
  key in obj
    ? new Map(Object.entries(obj)).get(key)
    : undefined;

/**
 * Validate an unknown value (config-borne) as a full
 * {@link Palette}. Iterates `schemes` × the token vocabulary
 * exhaustively — a missing scheme, a missing token, or a
 * bad hex is an `Err` naming the failing path (e.g.
 * `dark.danger-border`). Validates SHAPE, not taste: a
 * low-contrast override is the app's deliberate choice
 * (use `contrastRatio` to audit).
 */
export const asPalette = (
  value: unknown,
): Result<Palette, InvalidError> => {
  const cell = (
    scheme: Scheme,
    c: Color,
  ): Result<HexColor, InvalidError> => {
    const row = at(value, scheme);
    if (row === undefined) {
      return err(
        invalidError({
          message: `palette missing scheme: ${scheme}`,
        }),
      );
    }
    const cast = asHexColor(at(row, c));
    return isErr(cast)
      ? err(
          invalidError({
            message: `palette invalid at ${scheme}.${c}`,
          }),
        )
      : cast;
  };
  // First error wins; report the failing path.
  for (const scheme of schemes) {
    for (const c of colors) {
      const r = cell(scheme, c);
      if (isErr(r)) {
        return r;
      }
    }
  }
  // All cells valid — build via the typed literal shape.
  return ok(
    paletteOf((scheme, c) => {
      const r = cell(scheme, c);
      return isOk(r)
        ? r.content
        : mint("#000000");
    }),
  );
};

export { hex };
