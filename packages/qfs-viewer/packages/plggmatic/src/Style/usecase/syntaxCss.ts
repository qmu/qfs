import { type SoftStr } from "plgg";
import { type Scheme } from "plggmatic/Style/model/scheme";
import { hex } from "plggmatic/Style/model/hexColor";
import {
  type SyntaxKind,
  syntaxKinds,
  syntaxVar,
} from "plggmatic/Style/model/syntax";
import { type Theme } from "plggmatic/Style/model/theme";

/**
 * The `--<prefix>-code-*` custom-property declarations for
 * one scheme, as a single CSS body, in {@link syntaxKinds}
 * order. Values and namespace come from the {@link Theme}.
 */
const varsFor = (
  theme: Theme,
  scheme: Scheme,
): SoftStr =>
  syntaxKinds
    .map(
      (k: SyntaxKind) =>
        `--${theme.prefix}-code-${k}:${hex(theme.syntax[scheme][k])};`,
    )
    .join("");

/**
 * The `.tok-<kind>` class rules, one per colored kind, each
 * pointing at its `--<prefix>-code-*` property so the color
 * reschemes with `html.dark`. `comment` additionally carries
 * `font-style:italic`. Deliberately **unscoped** (no
 * `.vp-doc` ancestor): `tok-*` classes only ever appear on
 * plgg-highlight's spans, and an unscoped block is what lets
 * ANY consumer get themed code blocks for free. No rule
 * exists for `identifier`/`plain` — they inherit the code
 * block's default ink.
 */
const rulesFor = (theme: Theme): SoftStr =>
  syntaxKinds
    .map((k: SyntaxKind) =>
      k === "comment"
        ? `.tok-${k}{color:${syntaxVar(theme)(k)};font-style:italic}`
        : `.tok-${k}{color:${syntaxVar(theme)(k)}}`,
    )
    .join("");

/**
 * The engine's syntax-highlight stylesheet for a
 * {@link Theme}: the `--<prefix>-code-*` properties on
 * `:root` (light) with the `html.dark` override, followed
 * by the `.tok-*` class rules. Self-contained (defines the
 * properties it references) and escape-safe (no `<`, `>`,
 * `&`) so it survives an SSR text escaper byte-for-byte; a
 * host composes it into the document `<style>` alongside
 * `schemeCss`.
 */
export const syntaxCss = (
  theme: Theme,
): SoftStr =>
  `:root{${varsFor(theme, "light")}}html.dark{${varsFor(theme, "dark")}}` +
  rulesFor(theme);
