import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type Breakpoint,
  breakpoints,
  breakpointPx,
  minWidth,
  maxWidth,
} from "plggmatic/Style/model/breakpoint";

const SEEN: Record<Breakpoint, true> = {
  sm: true,
  snap: true,
  lg: true,
};

test("breakpoints lists its union once", () =>
  all([
    check(
      breakpoints.every((b) => SEEN[b]),
      toBe(true),
    ),
    check(
      breakpoints.length,
      toBe(new Set(breakpoints).size),
    ),
    check(breakpoints.length, toBe(3)),
  ]));

test("boundary widths match the oracle / example", () =>
  all([
    check(breakpointPx("sm"), toBe(640)),
    check(breakpointPx("snap"), toBe(900)),
    check(breakpointPx("lg"), toBe(1024)),
  ]));

test("min / max query builders match the oracle verbatim", () =>
  all([
    check(
      minWidth("snap"),
      toBe("(min-width:900px)"),
    ),
    check(
      minWidth("lg"),
      toBe("(min-width:1024px)"),
    ),
    check(
      maxWidth("sm"),
      toBe("(max-width:639px)"),
    ),
    check(
      maxWidth("snap"),
      toBe("(max-width:899px)"),
    ),
    check(
      maxWidth("lg"),
      toBe("(max-width:1023px)"),
    ),
  ]));

// The defining property: a max query is exactly one pixel
// below the min at the same breakpoint, so the pair never
// overlaps on the boundary pixel.
test("max query is min − 1px for every breakpoint", () =>
  all(
    breakpoints.map((b) =>
      check(
        maxWidth(b),
        toBe(
          `(max-width:${breakpointPx(b) - 1}px)`,
        ),
      ),
    ),
  ));
