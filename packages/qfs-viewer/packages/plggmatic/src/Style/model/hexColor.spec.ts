import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  asHexColor,
  isHexColor,
  hex,
} from "plggmatic/Style/model/hexColor";
import { isOk, isErr } from "plgg";

const mustHex = (s: string) => {
  const r = asHexColor(s);
  return isOk(r) ? r.content : undefined;
};

test("accepts #rrggbb and normalizes case", () => {
  const r = asHexColor("#AABBcc");
  return all([
    check(isOk(r), toBe(true)),
    check(
      isOk(r) ? hex(r.content) : "",
      toBe("#aabbcc"),
    ),
  ]);
});

test("rejects short, non-hex, and non-string values", () =>
  all([
    check(isErr(asHexColor("#abc")), toBe(true)),
    check(isErr(asHexColor("red")), toBe(true)),
    check(
      isErr(asHexColor("#gggggg")),
      toBe(true),
    ),
    check(isErr(asHexColor(123)), toBe(true)),
    check(
      isErr(asHexColor(undefined)),
      toBe(true),
    ),
  ]));

test("re-casting a HexColor is idempotent", () => {
  const first = mustHex("#123456");
  return check(
    first !== undefined &&
      isOk(asHexColor(first)),
    toBe(true),
  );
});

test("isHexColor distinguishes minted colors from strings", () => {
  const c = mustHex("#010203");
  return all([
    check(
      c !== undefined && isHexColor(c),
      toBe(true),
    ),
    check(isHexColor("#010203"), toBe(false)),
  ]);
});
