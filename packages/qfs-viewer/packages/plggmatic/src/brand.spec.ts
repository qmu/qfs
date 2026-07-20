import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { isOk, isErr } from "plgg";
import {
  pragmaticTheme,
  pragmaticThemeWithPalette,
} from "plggmatic/brand";
import { defaultPalette } from "plggmatic/styleEntry";

/**
 * plggmatic OWNS the branded `Theme` + palette-override API
 * (ticket A3, the empty-shell answer). These assertions
 * pin the substance: the branded default carries the `pm`
 * namespace and the preserved `vp-appearance` key (D16),
 * and the override API validates through the caster —
 * accepting a well-formed palette, rejecting a malformed
 * one as a value-level error (no throw, no `any`).
 */
test("pragmaticTheme is the branded default: pm prefix + vp-appearance key", () => {
  return all([
    check(pragmaticTheme.prefix, toBe("pm")),
    check(
      pragmaticTheme.storageKey,
      toBe("vp-appearance"),
    ),
  ]);
});

test("pragmaticThemeWithPalette validates a well-formed palette and layers it on the brand", () => {
  const result = pragmaticThemeWithPalette(
    defaultPalette,
  );
  return all([
    check(isOk(result), toBe(true)),
    // the non-palette fields stay the branded defaults
    check(
      isOk(result) ? result.content.prefix : "",
      toBe("pm"),
    ),
  ]);
});

test("pragmaticThemeWithPalette rejects a malformed palette as an InvalidError (no throw)", () => {
  const result = pragmaticThemeWithPalette({
    light: {},
    dark: {},
  });
  return check(isErr(result), toBe(true));
});
