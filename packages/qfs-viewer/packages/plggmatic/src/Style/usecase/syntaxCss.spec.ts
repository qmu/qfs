import {
  test,
  check,
  all,
  toBe,
  toContain,
  not,
} from "plgg-test";
import { syntaxKinds } from "plggmatic/Style/model/syntax";
import { syntaxCss as syntaxCssFor } from "plggmatic/Style/usecase/syntaxCss";
import { defaultTheme } from "plggmatic/Style/model/theme";

// The default theme reproduces the pre-parameterization
// `--pm-code-*` output byte-for-byte.
const syntaxCss = syntaxCssFor(defaultTheme);

const css: string = syntaxCss;

test("declares a --pm-code-* property for every kind, in both schemes", () =>
  all([
    check(
      css,
      toContain(":root{--pm-code-keyword:"),
    ),
    check(
      css,
      toContain("html.dark{--pm-code-keyword:"),
    ),
    // every colored kind is emitted as a property twice
    // (light + dark), so 14 --pm-code- occurrences total
    check(
      css.split("--pm-code-").length - 1,
      // 7 kinds × 2 scheme declarations + 7 var() refs in
      // the rules = 21 occurrences of the "--pm-code-" stem
      toBe(syntaxKinds.length * 3),
    ),
  ]));

test("maps each tok-<kind> class to its property; comment stays italic", () =>
  all([
    check(
      css,
      toContain(
        ".tok-keyword{color:var(--pm-code-keyword)}",
      ),
    ),
    check(
      css,
      toContain(
        ".tok-comment{color:var(--pm-code-comment);font-style:italic}",
      ),
    ),
    check(
      css,
      toContain(
        ".tok-punctuation{color:var(--pm-code-punctuation)}",
      ),
    ),
  ]));

test("rules are unscoped (consumer-agnostic) — no .vp-doc ancestor", () =>
  check(css, not(toContain(".vp-doc .tok-"))));

test("no rule or property exists for identifier or plain", () =>
  all([
    check(css, not(toContain("tok-identifier"))),
    check(css, not(toContain("tok-plain"))),
    check(css, not(toContain("code-identifier"))),
    check(css, not(toContain("code-plain"))),
  ]));

test("escape-safe: survives an SSR text escaper byte-for-byte", () =>
  all([
    check(css, not(toContain("<"))),
    check(css, not(toContain(">"))),
    check(css, not(toContain("&"))),
  ]));
