import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type ZBand,
  zBands,
  zValue,
} from "plggmatic/Style/model/zIndex";

const SEEN: Record<ZBand, true> = {
  content: true,
  chrome: true,
  backdrop: true,
  overlay: true,
};

test("zBands lists its union once", () =>
  all([
    check(
      zBands.every((b) => SEEN[b]),
      toBe(true),
    ),
    check(
      zBands.length,
      toBe(new Set(zBands).size),
    ),
    check(zBands.length, toBe(4)),
  ]));

test("bands are the oracle stack (1 / 30 / 40 / 50)", () =>
  all([
    check(zValue("content"), toBe(1)),
    check(zValue("chrome"), toBe(30)),
    check(zValue("backdrop"), toBe(40)),
    check(zValue("overlay"), toBe(50)),
  ]));

// Spaced for insertion: strictly increasing in declared
// order, so a new layer can slot between two without
// renumbering.
test("bands strictly increase content < chrome < backdrop < overlay", () =>
  all([
    check(
      zValue("content") < zValue("chrome"),
      toBe(true),
    ),
    check(
      zValue("chrome") < zValue("backdrop"),
      toBe(true),
    ),
    check(
      zValue("backdrop") < zValue("overlay"),
      toBe(true),
    ),
  ]));
