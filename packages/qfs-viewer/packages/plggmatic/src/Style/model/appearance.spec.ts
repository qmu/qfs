import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  appearanceStorageKey,
  decideScheme,
} from "plggmatic/Style/model/appearance";
import { some, none } from "plgg";

test("storage key is the preserved D16 value", () =>
  check(
    appearanceStorageKey,
    toBe("vp-appearance"),
  ));

test("decideScheme: a stored choice wins over OS preference", () =>
  all([
    check(
      decideScheme(some("dark"), false),
      toBe("dark"),
    ),
    check(
      decideScheme(some("light"), true),
      toBe("light"),
    ),
  ]));

test("decideScheme: absent or unknown falls back to OS preference", () =>
  all([
    check(
      decideScheme(none(), true),
      toBe("dark"),
    ),
    check(
      decideScheme(none(), false),
      toBe("light"),
    ),
    check(
      decideScheme(some("bogus"), true),
      toBe("dark"),
    ),
    check(
      decideScheme(some("bogus"), false),
      toBe("light"),
    ),
  ]));
