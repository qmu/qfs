import {
  test,
  check,
  all,
  toBe,
  toBeGreaterThanOrEqual,
} from "plgg-test";
import {
  type Color,
  colors,
  semanticRoles,
} from "plggmatic/Style/model/token";
import { colorHex } from "plggmatic/Style/model/palette";
import { contrastRatio } from "plggmatic/Style/usecase/contrast";
import { schemes } from "plggmatic/Style/model/scheme";
import {
  syntaxKinds,
  syntaxHex,
} from "plggmatic/Style/model/syntax";

// The accessibility gate, COMPUTED — not asserted by eye.
// This is the roadmap's phase-1 gate (D9): every text
// pairing clears WCAG 2.2 AA normal-text contrast
// (>= 4.5:1) and every semantic-role border clears the
// 1.4.11 non-text floor (>= 3:1), in BOTH schemes, across
// the full role×variant matrix, or this spec fails and the
// ticket cannot be approved. The WCAG math lives in the
// extracted `contrast` usecase (`contrastRatio`), which an
// override author can run over their own palette.

const pair = (
  fg: Color,
  bg: Color,
): readonly [Color, Color] => [fg, bg];

// Normal-text pairings (>= 4.5:1). Neutrals: body/secondary
// ink on both surfaces. Per role: the role ink on the two
// neutral surfaces and on the role's own tinted surface,
// plus the on-base label (the neutral `surface` token on
// the role's solid fill — the inverted-pill affordance).
const TEXT_PAIRS: ReadonlyArray<
  readonly [Color, Color]
> = [
  pair("text", "surface"),
  pair("text", "surface-2"),
  pair("muted", "surface"),
  pair("muted", "surface-2"),
  ...semanticRoles.flatMap((r) => [
    pair(`${r}-text`, "surface"),
    pair(`${r}-text`, "surface-2"),
    pair(`${r}-text`, `${r}-surface`),
    pair("surface", `${r}-base`),
  ]),
];

// Non-text pairings (>= 3:1): each semantic role's edge hue
// against both neutral surfaces (WCAG 1.4.11).
const BORDER_PAIRS: ReadonlyArray<
  readonly [Color, Color]
> = semanticRoles.flatMap((r) => [
  pair(`${r}-border`, "surface"),
  pair(`${r}-border`, "surface-2"),
]);

const AA_NORMAL = 4.5;
const AA_NON_TEXT = 3.0;

test("every text pairing clears WCAG AA (>=4.5:1) in both schemes", () =>
  all(
    schemes.flatMap((scheme) =>
      TEXT_PAIRS.map(([fg, bg]) =>
        check(
          contrastRatio(
            colorHex(scheme, fg),
            colorHex(scheme, bg),
          ),
          toBeGreaterThanOrEqual(AA_NORMAL),
        ),
      ),
    ),
  ));

test("every semantic border clears the non-text floor (>=3:1) in both schemes", () =>
  all(
    schemes.flatMap((scheme) =>
      BORDER_PAIRS.map(([fg, bg]) =>
        check(
          contrastRatio(
            colorHex(scheme, fg),
            colorHex(scheme, bg),
          ),
          toBeGreaterThanOrEqual(AA_NON_TEXT),
        ),
      ),
    ),
  ));

// Syntax hues (ticket 08): every colored SyntaxKind paints a
// `tok-*` span on the code-block surface `surface-2`, so each
// must clear AA normal-text (>=4.5:1) against surface-2 in
// its scheme (7 kinds × 2 schemes = 14 pairings). Derived
// from syntaxKinds so a new kind auto-extends the gate. This
// is why light `comment` carries the darkened default
// (#656d76): the oracle #6e7781 is 4.21:1 here.
test("every syntax hue clears WCAG AA (>=4.5:1) on surface-2 in both schemes", () =>
  all(
    schemes.flatMap((scheme) =>
      syntaxKinds.map((k) =>
        check(
          contrastRatio(
            syntaxHex(scheme, k),
            colorHex(scheme, "surface-2"),
          ),
          toBeGreaterThanOrEqual(AA_NORMAL),
        ),
      ),
    ),
  ));

// Coverage: every color token appears in at least one
// asserted pairing — EXCEPT the neutral `border`, the sole
// decorative hairline divider, whose legibility is not a
// contrast requirement (it is verified to be EMITTED by
// schemeCss.spec, not gated for ratio here).
test("every token except the neutral hairline is contrast-gated", () => {
  const covered = new Set<Color>();
  [...TEXT_PAIRS, ...BORDER_PAIRS].forEach(
    ([fg, bg]) => {
      covered.add(fg);
      covered.add(bg);
    },
  );
  return check(
    colors
      .filter((c) => c !== "border")
      .every((c) => covered.has(c)),
    toBe(true),
  );
});
