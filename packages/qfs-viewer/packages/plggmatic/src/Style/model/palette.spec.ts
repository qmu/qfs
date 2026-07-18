import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  asPalette,
  defaultPalette,
  colorHex,
  paletteHex,
  hex,
} from "plggmatic/Style/model/palette";
import { colors } from "plggmatic/Style/model/token";
import { schemes } from "plggmatic/Style/model/scheme";
import { isOk, isErr } from "plgg";

// A valid raw input (plain `{[k]:string}` objects) derived
// from the default palette — the shape config would supply.
const validInput = (): {
  light: Record<string, string>;
  dark: Record<string, string>;
} => {
  const row = (
    scheme: "light" | "dark",
  ): Record<string, string> =>
    Object.fromEntries(
      colors.map((c) => [
        c,
        hex(colorHex(scheme, c)),
      ]),
    );
  return {
    light: row("light"),
    dark: row("dark"),
  };
};

test("asPalette accepts a full valid palette (round-trips the default)", () =>
  all([
    check(
      isOk(asPalette(validInput())),
      toBe(true),
    ),
    check(
      isOk(asPalette(defaultPalette)),
      toBe(true),
    ),
  ]));

test("asPalette rejects a missing scheme", () =>
  check(
    isErr(
      asPalette({ light: validInput().light }),
    ),
    toBe(true),
  ));

test("asPalette rejects a missing token", () => {
  const v = validInput();
  delete v.light["primary-base"];
  return check(isErr(asPalette(v)), toBe(true));
});

test("asPalette rejects a malformed hex", () => {
  const v = validInput();
  v.dark["danger-border"] = "not-a-hex";
  return check(isErr(asPalette(v)), toBe(true));
});

test("colorHex/paletteHex agree with the default palette", () =>
  all(
    schemes.flatMap((scheme) =>
      colors.map((c) =>
        check(
          hex(colorHex(scheme, c)),
          toBe(
            hex(
              paletteHex(
                defaultPalette,
                scheme,
                c,
              ),
            ),
          ),
        ),
      ),
    ),
  ));
