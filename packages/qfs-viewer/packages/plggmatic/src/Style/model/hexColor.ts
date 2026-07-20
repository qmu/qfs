import {
  type Result,
  type InvalidError,
  type Box,
  invalidError,
  ok,
  err,
  isBoxWithTag,
  isSoftStr,
  box,
} from "plgg";

/**
 * A validated `#rrggbb` color literal — the only value the
 * scheme emitter accepts, so a palette can never carry a
 * malformed hex. Minted ONLY through {@link asHexColor}
 * (the `Box` brand keeps the constructor private), so an
 * arbitrary string cannot masquerade as a color. Follows
 * the `plgg/Basics/Str` brand+caster pattern.
 */
export type HexColor = Box<"HexColor", string>;

const HEX = /^#[0-9a-f]{6}$/;

const normalize = (
  value: unknown,
): string | undefined =>
  isSoftStr(value) &&
  HEX.test(value.toLowerCase())
    ? value.toLowerCase()
    : undefined;

const is = (value: unknown): value is HexColor =>
  isBoxWithTag("HexColor")(value) &&
  isSoftStr(value.content) &&
  HEX.test(value.content);

export const isHexColor = is;

/**
 * Cast an unknown value to a {@link HexColor}. Accepts a
 * case-insensitive `#rrggbb` string (normalized to
 * lowercase); anything else is an `Err` naming the
 * offending value.
 */
export const asHexColor = (
  value: unknown,
): Result<HexColor, InvalidError> => {
  if (is(value)) {
    return ok(value);
  }
  const norm = normalize(value);
  return norm === undefined
    ? err(
        invalidError({
          message: `Value is not a #rrggbb hex color: ${String(value)}`,
        }),
      )
    : ok(box("HexColor")(norm));
};

/**
 * The raw `#rrggbb` string of a {@link HexColor}, for the
 * emitter and the contrast math.
 */
export const hex = (c: HexColor): string =>
  c.content;
