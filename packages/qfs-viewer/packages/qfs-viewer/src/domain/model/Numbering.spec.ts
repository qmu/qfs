import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { formatOrdinal } from "#qfs-viewer/domain/model/Numbering";

// The ticket's worked example: "An `h3` under the second `h2` of the third
// `h1` renders the number `3-2-1.`". Measured against plgg-md 0.0.3, that
// heading's ordinal really is [3, 2, 1].
test("the ticket's example renders 3-2-1.", () =>
  check(
    formatOrdinal([3, 2, 1]),
    toBe("3-2-1."),
  ));

test("a top-level heading is 1.", () =>
  all([
    check(formatOrdinal([1]), toBe("1.")),
    check(formatOrdinal([3]), toBe("3.")),
  ]));

test("a two-level position is 1-2.", () =>
  check(formatOrdinal([1, 2]), toBe("1-2.")));

test("six levels deep still reads as one number", () =>
  check(
    formatOrdinal([1, 1, 1, 1, 1, 1]),
    toBe("1-1-1-1-1-1."),
  ));

// The reason zeroes are kept. plgg-md marks a skipped level with 0: `h1` then
// `h3` is [1, 0, 1]. Tidying that to "1-1." would collide with a real h2 under
// the first h1, which is ALSO [1, 1] -> "1-1.". Two positions, one number, and
// a citation that cannot tell them apart.
test("a skipped level keeps its zero and stays distinct from a real level", () =>
  all([
    check(
      formatOrdinal([1, 0, 1]),
      toBe("1-0-1."),
    ),
    check(formatOrdinal([1, 1]), toBe("1-1.")),
    // the collision the zero prevents
    check(
      formatOrdinal([1, 0, 1]) ===
        formatOrdinal([1, 1]),
      toBe(false),
    ),
  ]));

// A document with no h1, or one that opens at h2 — both real in this corpus,
// where plenty of documents start at "## Overview".
test("a document opening below h1 carries a leading zero", () =>
  all([
    check(formatOrdinal([0, 1]), toBe("0-1.")),
    check(formatOrdinal([0, 2]), toBe("0-2.")),
    check(
      formatOrdinal([0, 1, 1]),
      toBe("0-1-1."),
    ),
  ]));

// Total over its input type. No real heading can produce this — every heading
// has a level, so at least one slot — but the function does not need an
// assertion to say so.
test("an empty ordinal is the empty string, not a crash", () =>
  check(formatOrdinal([]), toBe("")));
