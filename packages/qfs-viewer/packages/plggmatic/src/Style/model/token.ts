import { type SoftStr } from "plgg";
// Type-only (erased): `Theme`'s `palette` field references
// `Color` from this module, so the two reference each
// other's types; there is no runtime edge back to `theme`.
import { type Theme } from "plggmatic/Style/model/theme";

/**
 * The closed color vocabulary. Because it is a union (not
 * a free string) an unknown role like `bg("blurple")`, or
 * a bare `bg("primary")` without a variant, is a compile
 * error — the type-driven win over stringly CSS classes.
 *
 * Per D9 (roadmap 2026-07-04,
 * `.workaholic/specs/20260704-plggpress-plggmatic-roadmap.md`)
 * the vocabulary is a MATRIX: five semantic roles ×
 * four variants (20 tokens), plus a five-member neutral
 * scale (25 tokens total), assembled from the exported
 * {@link SemanticRole}/{@link Variant}/{@link Neutral}
 * unions so the shape stays closed and adding a role
 * later is a single union-member edit whose fallout
 * (`colors`, `PALETTE`, the exhaustiveness pin, the
 * contrast pairs) is driven entirely by `tsc`.
 *
 * DOCTRINE (amended by D9): `token.ts` was originally a
 * deliberate *seed*, not a catalog — each role earned its
 * place from a concrete consumer. D9 fixes the role×variant
 * *shape* up front as roadmap vocabulary, so the
 * earned-place rule now applies one tier up: new ROLES
 * (secondary/tertiary) are still earned by a concrete
 * consumer and are deliberately NOT shipped yet.
 *
 * Variant semantics:
 * - `base` — the solid accent fill (button, active marker).
 *   Its on-fill label is the neutral `surface` token
 *   (inverted per scheme — exactly the qmu oracle's
 *   `--vp-hover`/`--vp-hover-ink` pair).
 * - `text` — the role's ink used as foreground on the
 *   neutral surfaces AND on the role's own `surface`.
 * - `surface` — the role-tinted panel background (a
 *   callout body).
 * - `border` — the role's edge hue (WCAG 1.4.11 non-text).
 *
 * Neutral scale:
 * - `surface` — the page/pane background prose sits on
 * - `surface-2` — a secondary panel (code block, table
 *   header, sunken rail) distinct from `surface`
 * - `text` — default body/heading ink on `surface`
 * - `muted` — secondary ink (captions, metadata); still
 *   meets AA on both surfaces (not decoration-only)
 * - `border` — hairline dividers and pane edges
 */
export type SemanticRole =
  | "primary"
  | "success"
  | "danger"
  | "warning"
  | "info";

export type Variant =
  "base" | "text" | "surface" | "border";

export type Neutral =
  | "surface"
  | "surface-2"
  | "text"
  | "muted"
  | "border";

export type Color =
  `${SemanticRole}-${Variant}` | Neutral;

export const semanticRoles: ReadonlyArray<SemanticRole> =
  [
    "primary",
    "success",
    "danger",
    "warning",
    "info",
  ];

export const variants: ReadonlyArray<Variant> = [
  "base",
  "text",
  "surface",
  "border",
];

export const neutrals: ReadonlyArray<Neutral> = [
  "surface",
  "surface-2",
  "text",
  "muted",
  "border",
];

/**
 * HOVER / HOVER-INK DECISION (D9 + ticket 05 — recorded,
 * not shipped as a token).
 *
 * The qmu signature affordance — the inverted pill on
 * every chrome link and active nav leaf — needs a fill
 * (`--vp-hover`) and its label ink (`--vp-hover-ink`).
 * Under the monochrome default this pair is EXACTLY the
 * on-base-label convention already pinned by the matrix,
 * so no dedicated token ships:
 *
 * - `hover`     := `primary-base` (light `#111111`, dark
 *   `#f4f4f4` — the oracle's dark `rgba(255,255,255,.95)`
 *   alpha-flattened per the palette's step 4).
 * - `hover-ink` := neutral `surface` (light `#ffffff`,
 *   dark `#1b1b1f`).
 *
 * i.e. the inverted pill is `neutral surface` painted on
 * `primary-base` — the same two values a `primary-base`
 * fill already labels with. A second name would be a
 * synonym, so D9's earned-place rule (at the role tier)
 * keeps it derived.
 *
 * REVISIT TRIGGERS:
 * 1. Ticket 04's palette-override API — a non-black
 *    `primary-base` turns the monochrome pill into a
 *    COLORED pill. If qmu identity requires the inversion
 *    to stay monochrome under an overridden palette, the
 *    pair earns its own token then (and joins the ticket-03
 *    contrast gate in both schemes).
 * 2. Ticket 07's port — if any inverted surface fails AA
 *    under this derivation, the contrast spec is the
 *    arbiter and the pair is re-decided.
 *
 * This is a THEME idiom expressed through existing tokens.
 * It is distinct from the COMPONENT hover-feedback rule
 * (an opacity dim — see `Component/model/interaction.ts`
 * `hoverDim`): that governs a control dimming on `:hover`,
 * this governs a link/leaf inverting its fill. Both stand.
 */

/**
 * Every {@link Color}, DERIVED from the unions so the list
 * can never drift from the type. The scheme emitter and
 * specs iterate this; the exhaustiveness spin in
 * `token.spec.ts` pins it to the union at compile time.
 */
export const colors: ReadonlyArray<Color> = [
  ...semanticRoles.flatMap(
    (r): ReadonlyArray<Color> =>
      variants.map((v): Color => `${r}-${v}`),
  ),
  ...neutrals,
];

/**
 * The concrete hex for every token in every scheme. The
 * `Record<Scheme, Record<Color, SoftStr>>` shape makes
 * this exhaustive twice over: a missing scheme or a
 * missing token is a `tsc` error, so the palette can never
 * ship a hole. Literal color values live ONLY here — every
 * atom resolves through {@link colorVar}.
 *
 * The default is MONOCHROME (D9): the qmu.co.jp oracle port
 * carried in `plggpress/src/theme/baseCss.ts` — black
 * `#111111` on white `#ffffff` in light, near-white on
 * near-black in dark. The dark neutral inks are the
 * oracle's translucent whites flattened over the dark
 * `surface` (single solid hex is required by the hex-shape
 * spec and the emitter): text `rgba(240,240,245,.92)` →
 * `#dfdfe4`, muted `rgba(235,235,245,.55)` → `#8d8d95`,
 * brand `rgba(255,255,255,.95)` → `#f4f4f4`. Semantic
 * surfaces/inks are seeded from the oracle callout hues;
 * `base`/`border` tiers are chosen so the on-base label
 * and the 3:1 border floor clear AA (the contrast spec is
 * the arbiter). `info` has no oracle value (plggpress
 * renders info in brand) — a provisional blue family,
 * flagged as the one non-oracle role. The palette DATA
 * (the default values, the override caster) lives in
 * `Style/model/palette.ts`; `token.ts` owns only the token
 * *vocabulary* and `colorVar`, so overriding a palette
 * never reaches this file.
 */

/**
 * The `var(--<prefix>-<token>)` reference for a token —
 * what every color atom emits, so the value is resolved by
 * the active scheme's custom properties at paint time
 * rather than baked in. The namespace prefix comes from the
 * supplied {@link Theme} (`pm` by default), so the emitted
 * `:root` block and the atoms that reference it share one
 * namespace. Curried `colorVar(theme)(c)` so a theme-bound
 * emitter (or the default-bound atoms) fixes the prefix
 * once.
 */
export const colorVar =
  (theme: Theme) =>
  (c: Color): SoftStr =>
    `var(--${theme.prefix}-${c})`;
