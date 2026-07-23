import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { isHexColor } from "plggmatic/Style/model/hexColor";
import { schemes } from "plggmatic/Style/model/scheme";
import {
  type SyntaxKind,
  syntaxKinds,
  syntaxHex,
  syntaxVar as syntaxVarFor,
} from "plggmatic/Style/model/syntax";
import { defaultTheme } from "plggmatic/Style/model/theme";

// `syntaxVar(theme)(k)` under the default theme is exactly
// the old `syntaxVar(k)` — `var(--pm-code-<kind>)`.
const syntaxVar = syntaxVarFor(defaultTheme);

// Compile-time exhaustiveness pin on the SOURCE union: a
// member added to SyntaxKind but missing from the array
// (or vice versa) fails `tsc` before any test runs. Mirrors
// token.spec.ts's SEEN_ROLE pattern.
const SEEN: Record<SyntaxKind, true> = {
  keyword: true,
  string: true,
  number: true,
  comment: true,
  regex: true,
  template: true,
  punctuation: true,
};

test("syntaxKinds lists its union once (seven colored kinds)", () =>
  all([
    check(
      syntaxKinds.every((k) => SEEN[k]),
      toBe(true),
    ),
    check(syntaxKinds.length, toBe(7)),
    check(
      syntaxKinds.length,
      toBe(new Set(syntaxKinds).size),
    ),
  ]));

test("identifier and plain are deliberately absent (they inherit the default ink)", () => {
  // widen to strings (no cast — SyntaxKind is assignable to
  // string) so the two uncolored kinds can be checked absent
  const names: ReadonlyArray<string> =
    syntaxKinds.map((k): string => k);
  return all([
    check(
      names.includes("identifier"),
      toBe(false),
    ),
    check(names.includes("plain"), toBe(false)),
  ]);
});

test("every palette value is a valid #rrggbb hex in both schemes", () =>
  check(
    schemes.every((s) =>
      syntaxKinds.every((k) =>
        isHexColor(syntaxHex(s, k)),
      ),
    ),
    toBe(true),
  ));

test("syntaxVar references the --pm-code namespace", () =>
  all([
    check(
      syntaxVar("keyword"),
      toBe("var(--pm-code-keyword)"),
    ),
    check(
      syntaxVar("comment"),
      toBe("var(--pm-code-comment)"),
    ),
  ]));

test("light comment is contrast-darkened from the oracle (#6e7781 -> #656d76)", () =>
  all([
    // the recorded deviation: the oracle gray fails AA on the
    // code surface, so the default carries the darkened value
    check(
      syntaxHex("light", "comment").content,
      toBe("#656d76"),
    ),
    // every other light hue keeps the oracle byte-for-byte
    check(
      syntaxHex("light", "keyword").content,
      toBe("#cf222e"),
    ),
    check(
      syntaxHex("dark", "keyword").content,
      toBe("#ff7b72"),
    ),
  ]));
