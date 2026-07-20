import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { matchOption } from "plgg";
import {
  type TypeRole,
  type FontWeight,
  type CompactType,
  typeRoles,
  fontWeights,
  regular,
  medium,
  semibold,
  typeScale,
  sansFontStack,
} from "plggmatic/Style/model/typography";

// Compile-time exhaustiveness pins on the SOURCE unions:
// a member added to a union but missing from its array
// (or vice versa) fails `tsc` before any test runs.
const SEEN_ROLE: Record<TypeRole, true> = {
  h1: true,
  h2: true,
  h3: true,
  h4: true,
  body: true,
};
const SEEN_WEIGHT: Record<FontWeight, true> = {
  400: true,
  500: true,
  600: true,
};

test("typeRoles / fontWeights list their unions once", () =>
  all([
    check(
      typeRoles.every((r) => SEEN_ROLE[r]),
      toBe(true),
    ),
    check(
      typeRoles.length,
      toBe(new Set(typeRoles).size),
    ),
    check(
      fontWeights.every((w) => SEEN_WEIGHT[w]),
      toBe(true),
    ),
    check(
      fontWeights.length,
      toBe(new Set(fontWeights).size),
    ),
  ]));

test("weight tokens are the oracle 400/500/600", () =>
  all([
    check(regular, toBe(400)),
    check(medium, toBe(500)),
    check(semibold, toBe(600)),
  ]));

test("type scale matches the oracle size / leading / weight", () =>
  all([
    check(typeScale.h1.size, toBe("1.875rem")),
    check(typeScale.h1.lineHeight, toBe("1.25")),
    check(typeScale.h1.weight, toBe(400)),
    check(typeScale.h2.size, toBe("1.5rem")),
    check(typeScale.h2.lineHeight, toBe("1.3")),
    check(typeScale.h2.weight, toBe(400)),
    check(typeScale.h3.size, toBe("1.1875rem")),
    check(typeScale.h3.lineHeight, toBe("1.45")),
    check(typeScale.h4.size, toBe("1.0625rem")),
    check(typeScale.h4.lineHeight, toBe("1.5")),
    check(typeScale.body.size, toBe("1rem")),
    check(
      typeScale.body.lineHeight,
      toBe("1.75"),
    ),
    check(typeScale.body.weight, toBe(400)),
  ]));

// Every role's declared weight is a member of the closed
// FontWeight set (belt-and-braces against a stray value).
test("every role weight is in the FontWeight set", () =>
  all(
    typeRoles.map((r) =>
      check(
        SEEN_WEIGHT[typeScale[r].weight],
        toBe(true),
      ),
    ),
  ));

// Value-shape: sizes are rem, line-heights are unitless
// (inheritance-safe) — the Considerations note that the
// shape spec must not force everything into rem.
test("sizes are rem, line-heights unitless", () =>
  all(
    typeRoles.flatMap((r) => [
      check(
        typeScale[r].size.endsWith("rem"),
        toBe(true),
      ),
      check(
        /^[0-9.]+$/.test(typeScale[r].lineHeight),
        toBe(true),
      ),
    ]),
  ));

// The compact (sub-`sm`) overrides: h1/h2 narrow size AND
// re-state leading, h3 narrows size only (lineHeight
// `None`), h4/body have no compact variant (`None`).
const compactSize = (role: TypeRole): string =>
  matchOption<CompactType, string>(
    () => "none",
    (c) => c.size,
  )(typeScale[role].compact);

const compactLeading = (role: TypeRole): string =>
  matchOption<CompactType, string>(
    () => "none",
    (c) =>
      matchOption<string, string>(
        () => "none",
        (lh) => lh,
      )(c.lineHeight),
  )(typeScale[role].compact);

test("compact overrides match the oracle phone-column block", () =>
  all([
    check(compactSize("h1"), toBe("1.75rem")),
    check(compactLeading("h1"), toBe("1.25")),
    check(compactSize("h2"), toBe("1.375rem")),
    check(compactLeading("h2"), toBe("1.3")),
    check(compactSize("h3"), toBe("1.125rem")),
    // h3 keeps its base leading — no compact line-height.
    check(compactLeading("h3"), toBe("none")),
    // h4 / body have no compact variant at all.
    check(compactSize("h4"), toBe("none")),
    check(compactSize("body"), toBe("none")),
  ]));

test("sans font stack is Inter-first and escape-safe", () =>
  all([
    check(
      sansFontStack.startsWith('"Inter"'),
      toBe(true),
    ),
    check(
      sansFontStack.includes("<"),
      toBe(false),
    ),
    check(
      sansFontStack.includes(">"),
      toBe(false),
    ),
    check(
      sansFontStack.includes("&"),
      toBe(false),
    ),
  ]));
