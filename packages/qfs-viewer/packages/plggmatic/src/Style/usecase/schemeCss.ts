import { type SoftStr } from "plgg";
import { type Scheme } from "plggmatic/Style/model/scheme";
import { colors } from "plggmatic/Style/model/token";
import {
  type Palette,
  paletteHex,
  hex,
} from "plggmatic/Style/model/palette";
import {
  type Theme,
  defaultTheme,
} from "plggmatic/Style/model/theme";

/**
 * The `--<prefix>-*` custom-property declarations for one
 * scheme of a palette, as a single CSS body
 * (`--pm-surface:#…;…`). Every token is emitted, in
 * {@link colors} order. The namespace prefix is supplied
 * by the caller (from the {@link Theme}) so the block and
 * the atoms that reference it agree.
 */
const varsFor = (
  palette: Palette,
  scheme: Scheme,
  prefix: SoftStr,
): SoftStr =>
  colors
    .map(
      (c) =>
        `--${prefix}-${c}:${hex(paletteHex(palette, scheme, c))};`,
    )
    .join("");

/**
 * The engine's base color stylesheet for a {@link Theme}:
 * the light scheme on `:root` and the dark override under
 * `html.dark`, toggled by one `dark` class. `html.dark` is
 * the single published scheme mechanism (no attribute
 * variants). Escape-safe (no `<`, `>`, `&`) so it survives
 * an SSR text escaper byte-for-byte. Inject ahead of the
 * collected atomic rules so the custom properties are
 * defined before any `var(--<prefix>-*)` resolves. The
 * consumer passes its theme (`schemeCss(defaultTheme)` for
 * the monochrome default) at its composition root.
 */
export const schemeCss = (
  theme: Theme,
): SoftStr =>
  `:root{${varsFor(theme.palette, "light", theme.prefix)}}html.dark{${varsFor(theme.palette, "dark", theme.prefix)}}`;

/**
 * Scheme CSS for a bare palette under the default prefix —
 * the palette-override convenience: `schemeCssOf(palette)`
 * is `schemeCss` over the default theme with `palette`
 * swapped in, so an override that only changes colors need
 * not build a whole {@link Theme}.
 */
export const schemeCssOf = (
  palette: Palette,
): SoftStr =>
  schemeCss({ ...defaultTheme, palette });
