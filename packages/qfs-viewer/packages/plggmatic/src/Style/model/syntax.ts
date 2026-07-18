import { type SoftStr, box } from "plgg";
import { type Scheme } from "plggmatic/Style/model/scheme";
import { type HexColor } from "plggmatic/Style/model/hexColor";
import { type Theme } from "plggmatic/Style/model/theme";

/**
 * The closed syntax-highlight vocabulary — a SIBLING of the
 * {@link Color} token matrix (`Style/model/token.ts`), NOT
 * new members of it: `bg("code-keyword")` stays a compile
 * error, because syntax hues paint plgg-highlight's
 * `tok-<kind>` spans through their own `--pm-code-*`
 * properties, never the color atoms. A closed union so an
 * unknown kind is a `tsc` error and the palette / emitter
 * stay exhaustive.
 *
 * Seven of plgg-highlight's nine `TokenKind`s are colored;
 * **`identifier` and `plain` are deliberately absent** — they
 * inherit the code block's default ink (the pre-existing
 * behavior, kept on purpose), so plggmatic emits neither a
 * `--pm-code-*` property nor a `.tok-*` rule for them.
 *
 * **Pinned contract with plgg-highlight.** These kind names
 * MUST equal plgg-highlight's `tok-<kind>` class stems
 * (`Render/usecase/highlight.ts` `tokenClass`). plggmatic
 * does NOT import plgg-highlight to learn them (its deps
 * stay `plgg` + `plgg-view`); the name agreement is pinned
 * by a cross-package spec in plggpress — the one package
 * that depends on both sides of the seam.
 */
export type SyntaxKind =
  | "keyword"
  | "string"
  | "number"
  | "comment"
  | "regex"
  | "template"
  | "punctuation";

export const syntaxKinds: ReadonlyArray<SyntaxKind> =
  [
    "keyword",
    "string",
    "number",
    "comment",
    "regex",
    "template",
    "punctuation",
  ];

// The default values follow the oracle discipline: the
// shipped GitHub-palette hexes plggpress `baseCss` hardcoded,
// adopted verbatim EXCEPT where the computed contrast gate
// (`contrast.spec.ts`, extended for syntax) forces a change:
//
//   light `comment` #6e7781 → #656d76 — the oracle gray is
//   4.21:1 on the code surface-2 (#f6f6f7), below AA 4.5:1;
//   darkened minimally to 4.86:1 (the same contrast-forced
//   move ticket 03 made for `muted`). Every other oracle hex
//   already clears 4.5:1 on its scheme's surface-2, so it is
//   kept byte-for-byte.
const RAW: Record<
  Scheme,
  Record<SyntaxKind, string>
> = {
  light: {
    keyword: "#cf222e",
    string: "#0a3069",
    number: "#0550ae",
    comment: "#656d76",
    regex: "#116329",
    template: "#0a3069",
    punctuation: "#57606a",
  },
  dark: {
    keyword: "#ff7b72",
    string: "#a5d6ff",
    number: "#79c0ff",
    comment: "#8b949e",
    regex: "#7ee787",
    template: "#a5d6ff",
    punctuation: "#c9d1d9",
  },
};

// A scheme's row as a typed 7-key LITERAL (no `as`), so a
// missing kind is a `tsc` error — the exhaustive-twice-over
// shape `palette.ts` uses, parameterized by a getter.
const rowOf = (
  get: (k: SyntaxKind) => HexColor,
): Record<SyntaxKind, HexColor> => ({
  keyword: get("keyword"),
  string: get("string"),
  number: get("number"),
  comment: get("comment"),
  regex: get("regex"),
  template: get("template"),
  punctuation: get("punctuation"),
});

// Direct mint of the compile-time-known-good defaults; the
// `syntax.spec` isHex check over every value is the guard,
// so no dead Err branch. `box` is a plgg constructor, not an
// escape hatch (mirrors `palette.ts`'s `mint`).
const mint = (s: string): HexColor =>
  box("HexColor")(s);

/**
 * The default syntax palette: every {@link SyntaxKind} in
 * every {@link Scheme} as a validated {@link HexColor}. The
 * `Record<Scheme, Record<SyntaxKind, HexColor>>` shape is
 * exhaustive twice over — a missing scheme or kind is a
 * `tsc` error. Shaped as a Record so ticket 04's
 * palette-override API can reach syntax the same way it
 * reaches the color matrix (a consumer overriding `primary`
 * can override `keyword`); the config→override wiring rides
 * the schemeCss precedent and is added by the consumer
 * tickets.
 */
export const syntaxPalette: Record<
  Scheme,
  Record<SyntaxKind, HexColor>
> = {
  light: rowOf((k) => mint(RAW.light[k])),
  dark: rowOf((k) => mint(RAW.dark[k])),
};

/** A syntax kind's {@link HexColor} in a scheme. */
export const syntaxHex = (
  scheme: Scheme,
  k: SyntaxKind,
): HexColor => syntaxPalette[scheme][k];

/**
 * The `var(--pm-code-<kind>)` reference for a kind — what
 * the `.tok-<kind>` rule emits, resolved per scheme by the
 * `--pm-code-*` properties. The `pm` namespace comes from
 * {@link cssPrefix}; the `code-` segment separates syntax
 * hues from the `--pm-<color>` token namespace.
 */
export const syntaxVar =
  (theme: Theme) =>
  (k: SyntaxKind): SoftStr =>
    `var(--${theme.prefix}-code-${k})`;
