import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  renderToString,
  collectCss,
  text,
  p as para,
} from "plgg-view";
import {
  heading,
  prose,
} from "plggmatic/Component/usecase/typography";

test("heading level maps to the real hN element", () =>
  all([
    check(
      renderToString(
        heading(1, "Title"),
      ).startsWith("<h1"),
      toBe(true),
    ),
    check(
      renderToString(
        heading(2, "Title"),
      ).startsWith("<h2"),
      toBe(true),
    ),
    check(
      renderToString(
        heading(3, "Title"),
      ).startsWith("<h3"),
      toBe(true),
    ),
    check(
      renderToString(
        heading(4, "Title"),
      ).startsWith("<h4"),
      toBe(true),
    ),
  ]));

test("heading renders the guide type scale (size, weight 400, leading)", () =>
  all([
    check(
      collectCss(heading(1, "T")).includes(
        "font-size:1.875rem",
      ),
      toBe(true),
    ),
    check(
      collectCss(heading(1, "T")).includes(
        "font-weight:400",
      ),
      toBe(true),
    ),
    check(
      collectCss(heading(1, "T")).includes(
        "line-height:1.25",
      ),
      toBe(true),
    ),
    check(
      collectCss(heading(4, "T")).includes(
        "font-size:1.0625rem",
      ),
      toBe(true),
    ),
  ]));

test("prose caps the reading measure at the metric var", () =>
  all([
    check(
      renderToString(
        prose([para([], [text("body")])]),
      ).startsWith("<div"),
      toBe(true),
    ),
    check(
      collectCss(
        prose([para([], [text("body")])]),
      ).includes("max-width:var(--pm-measure)"),
      toBe(true),
    ),
    check(
      collectCss(
        prose([para([], [text("body")])]),
      ).includes("line-height:1.75"),
      toBe(true),
    ),
  ]));

test("typography is pure", () =>
  all([
    check(
      renderToString(heading(2, "Title")),
      toBe(renderToString(heading(2, "Title"))),
    ),
    check(
      renderToString(prose([text("x")])),
      toBe(renderToString(prose([text("x")]))),
    ),
  ]));
