import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  modes,
  toggleMode,
} from "plggmatic/Render/model/mode";

test("the two modes are enumerated", () =>
  check(
    modes,
    toEqual(["multiColumn", "singleColumn"]),
  ));

test("toggleMode flips between the modes", () =>
  all([
    check(
      toggleMode("multiColumn"),
      toBe("singleColumn"),
    ),
    check(
      toggleMode("singleColumn"),
      toBe("multiColumn"),
    ),
  ]));

test("toggling twice is the identity", () =>
  check(
    toggleMode(toggleMode("multiColumn")),
    toBe("multiColumn"),
  ));
