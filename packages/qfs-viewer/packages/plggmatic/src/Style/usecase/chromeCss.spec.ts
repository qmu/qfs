import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { chromeCss as chromeCssFor } from "plggmatic/Style/usecase/chromeCss";
import { defaultTheme } from "plggmatic/Style/model/theme";

// The default theme reproduces the pre-parameterization
// chrome CSS byte-for-byte.
const chromeCss = chromeCssFor(defaultTheme);

test("the chrome CSS is escape-safe (survives an SSR escaper)", () =>
  all([
    check(chromeCss.includes("<"), toBe(false)),
    check(chromeCss.includes(">"), toBe(false)),
    check(chromeCss.includes("&"), toBe(false)),
  ]));

test("media boundaries come from the snap breakpoint builders", () =>
  all([
    check(
      chromeCss.includes("(min-width:900px)"),
      toBe(true),
    ),
    check(
      chromeCss.includes("(max-width:899px)"),
      toBe(true),
    ),
  ]));

test("the below-snap strip is a mandatory one-column-per-swipe left-edge snap", () =>
  all([
    // firm (mandatory), not the soft proximity snap
    check(
      chromeCss.includes(
        "scroll-snap-type:x mandatory",
      ),
      toBe(true),
    ),
    check(
      chromeCss.includes(
        "scroll-snap-type:x proximity",
      ),
      toBe(false),
    ),
    // one column per swipe, aligned to the left edge
    check(
      chromeCss.includes(
        "scroll-snap-align:start",
      ),
      toBe(true),
    ),
    check(
      chromeCss.includes(
        "scroll-snap-stop:always",
      ),
      toBe(true),
    ),
    // the swipe stays contained (no browser back-gesture)
    check(
      chromeCss.includes(
        "overscroll-behavior-x:contain",
      ),
      toBe(true),
    ),
    // trailing runway (a real spacer box, not a margin —
    // margins are unreliably counted in scroll width) so even
    // a shallow last column reaches the left edge
    check(
      chromeCss.includes(
        '.pm-row::after{content:"";flex:0 0 100vw;}',
      ),
      toBe(true),
    ),
  ]));

test("colors are --pm-* variables and dimensions are tokens", () =>
  all([
    check(
      chromeCss.includes("var(--pm-surface)"),
      toBe(true),
    ),
    check(
      chromeCss.includes(
        "var(--pm-primary-base)",
      ),
      toBe(true),
    ),
    // per-column scroll uses the chrome-rail metric var,
    // never a raw 48px literal
    check(
      chromeCss.includes("var(--pm-rail)"),
      toBe(true),
    ),
    check(
      chromeCss.includes("48px"),
      toBe(false),
    ),
  ]));

test("class hooks are cssPrefix-derived", () =>
  all([
    check(
      chromeCss.includes(".pm-colhead"),
      toBe(true),
    ),
    check(
      chromeCss.includes(
        '.pm-pane a[aria-current="page"]',
      ),
      toBe(true),
    ),
    check(
      chromeCss.includes(".ex-"),
      toBe(false),
    ),
  ]));
