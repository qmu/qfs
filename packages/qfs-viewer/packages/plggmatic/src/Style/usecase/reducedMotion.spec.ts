import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { reducedMotionCss } from "plggmatic/Style/usecase/reducedMotion";

test("resets smooth scrolling to auto under reduced motion", () =>
  all([
    check(
      reducedMotionCss.includes(
        "@media (prefers-reduced-motion:reduce)",
      ),
      toBe(true),
    ),
    check(
      reducedMotionCss.includes(
        "scroll-behavior:auto",
      ),
      toBe(true),
    ),
    check(
      reducedMotionCss.includes("html{"),
      toBe(true),
    ),
    check(
      reducedMotionCss.includes("main{"),
      toBe(true),
    ),
  ]));

test("reduced-motion CSS is escape-safe (no <, >, &)", () =>
  all([
    check(
      reducedMotionCss.includes("<"),
      toBe(false),
    ),
    check(
      reducedMotionCss.includes(">"),
      toBe(false),
    ),
    check(
      reducedMotionCss.includes("&"),
      toBe(false),
    ),
  ]));
