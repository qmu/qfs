// From the engine's Style barrel, NOT `styleEntry`:
// `styleEntry` re-exports this module, so importing it
// here would close an import cycle.
import {
  type Theme,
  type Palette,
  defaultTheme,
  asPalette,
} from "plggmatic/Style";
import {
  type Result,
  type InvalidError,
  ok,
  err,
  matchResult,
} from "plgg";

/**
 * The Pragmatic design system's branded default `Theme`:
 * the monochrome `--pm-*` design language, owned and
 * versioned HERE. The engine's `Style` tier ships the
 * value-identical but brand-neutral {@link defaultTheme};
 * this module is its brand home. This is the design-system
 * substance the `plggmatic` package carries — the answer
 * to the "empty shell" risk (trip
 * `plggmatic-extraction-cut`, ticket A3): the package is a
 * real, versionable owner of the `Theme` contract and the
 * palette-override API, not a bare re-export label.
 */
export const pragmaticTheme: Theme = defaultTheme;

/**
 * The palette-override API: validate a caller-supplied
 * palette through the `asPalette` caster (ticket 04's
 * contract — never widened to `any`; a hole or malformed
 * hex is a value-level {@link InvalidError}, not a throw)
 * and layer it onto the branded theme, keeping the
 * prefix / metrics / typeScale / syntax / zBands /
 * storageKey. The one set of palette bytes lives in the
 * engine `Style` tier's `defaultTheme`; a brand override
 * rebuilds only the `palette` field — no duplication, and
 * plggpress never reaches this API.
 */
export const pragmaticThemeWithPalette = (
  paletteInput: unknown,
): Result<Theme, InvalidError> =>
  matchResult<
    Palette,
    InvalidError,
    Result<Theme, InvalidError>
  >(
    (e) => err(e),
    (palette) =>
      ok({ ...pragmaticTheme, palette }),
  )(asPalette(paletteInput));
