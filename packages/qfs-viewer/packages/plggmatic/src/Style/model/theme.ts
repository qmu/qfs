import { type SoftStr } from "plgg";
import { cssPrefix } from "plggmatic/Meta/model/identity";
import {
  type Palette,
  defaultPalette,
} from "plggmatic/Style/model/palette";
import {
  type Metric,
  metricTable,
} from "plggmatic/Style/model/metric";
import {
  type TypeRole,
  type TypeScale,
  typeScale,
} from "plggmatic/Style/model/typography";
import { type Scheme } from "plggmatic/Style/model/scheme";
import {
  type SyntaxKind,
  syntaxPalette,
} from "plggmatic/Style/model/syntax";
import { type HexColor } from "plggmatic/Style/model/hexColor";
import {
  type ZBand,
  zBandTable,
} from "plggmatic/Style/model/zIndex";
import { appearanceStorageKey } from "plggmatic/Style/model/appearance";

/**
 * The design language as a single closed record — the
 * empty-shell answer of the `plggmatic-extraction-cut`
 * trip (ticket A3). Every value the CSS emitters and color
 * atoms used to close over as a module constant is now a
 * field here, so a consumer supplies its inputs explicitly
 * instead of the engine baking one brand in:
 *
 * - `prefix`     — the custom-property namespace
 *   (`--<prefix>-*`); `pm` by default. `colorVar`/
 *   `metricVar`/the emitters read it, so the emitted block
 *   and the atoms that reference it agree by construction.
 * - `palette`    — every color token's hex, per scheme
 *   (the `schemeCss` source; the override API's target).
 * - `metrics`    — the shell-geometry lengths (`metricCss`).
 * - `typeScale`  — the prose type roles.
 * - `syntax`     — the code-token palette (`syntaxCss`).
 * - `zBands`     — the stacking-band integers.
 * - `storageKey` — the appearance persistence key
 *   (`vp-appearance` by default — a visitor's saved scheme
 *   must survive the extraction, D16).
 *
 * A widened or missing field is a `tsc` error — no `as`,
 * no `any`. The engine ships {@link defaultTheme} (values
 * = today's monochrome `--pm-*` design language) as its
 * brand-neutral default; `plggmatic/brand` owns the
 * BRANDED instance and the palette-override API layered
 * over this contract.
 */
export type Theme = Readonly<{
  prefix: SoftStr;
  palette: Palette;
  metrics: Record<Metric, SoftStr>;
  typeScale: Record<TypeRole, TypeScale>;
  syntax: Record<
    Scheme,
    Record<SyntaxKind, HexColor>
  >;
  zBands: Record<ZBand, number>;
  storageKey: SoftStr;
}>;

/**
 * The neutral default `Theme`: the monochrome `--pm-*`
 * design language the engine shipped before
 * parameterization (prefix `pm`, the oracle palette,
 * `vp-appearance`), so passing `defaultTheme` to the
 * emitters reproduces the old output byte-for-byte
 * (D3/D16). `plggmatic/brand` re-brands this as
 * Pragmatic's; `plggpress` passes it at its composition
 * root and may diverge later.
 */
export const defaultTheme: Theme = {
  prefix: cssPrefix,
  palette: defaultPalette,
  metrics: metricTable,
  typeScale: typeScale,
  syntax: syntaxPalette,
  zBands: zBandTable,
  storageKey: appearanceStorageKey,
};
