import {
  test,
  check,
  all,
  toEqual,
} from "plgg-test";
import {
  bg,
  color,
  textColor,
  border,
  borderColor,
  outline,
  basis,
  fluid,
  lineHeight,
  weight,
  zIndex,
  typeStyle,
  measure,
} from "plggmatic/Style/usecase/utilities";

test("color atoms resolve through --pm-* vars", () =>
  all([
    check(
      bg("surface"),
      toEqual([
        {
          prop: "background-color",
          value: "var(--pm-surface)",
        },
      ]),
    ),
    check(
      color("text"),
      toEqual([
        {
          prop: "color",
          value: "var(--pm-text)",
        },
      ]),
    ),
    check(
      textColor("muted"),
      toEqual([
        {
          prop: "color",
          value: "var(--pm-muted)",
        },
      ]),
    ),
    check(
      borderColor("primary-base"),
      toEqual([
        {
          prop: "border-color",
          value: "var(--pm-primary-base)",
        },
      ]),
    ),
    check(
      outline("primary-base"),
      toEqual([
        {
          prop: "outline",
          value:
            "2px solid var(--pm-primary-base)",
        },
      ]),
    ),
  ]));

test("border is a three-atom hairline in the var", () =>
  check(
    border,
    toEqual([
      { prop: "border-width", value: "1px" },
      { prop: "border-style", value: "solid" },
      {
        prop: "border-color",
        value: "var(--pm-border)",
      },
    ]),
  ));

test("column tracks are whole-shorthand atoms", () =>
  all([
    check(
      basis("220px"),
      toEqual([
        { prop: "flex", value: "0 0 220px" },
        { prop: "width", value: "220px" },
      ]),
    ),
    check(
      fluid,
      toEqual([
        { prop: "flex", value: "1 1 auto" },
        { prop: "min-width", value: "0" },
      ]),
    ),
  ]));

test("line-height / weight / z-index atoms", () =>
  all([
    check(
      lineHeight("1.75"),
      toEqual([
        { prop: "line-height", value: "1.75" },
      ]),
    ),
    check(
      weight(500),
      toEqual([
        { prop: "font-weight", value: "500" },
      ]),
    ),
    check(
      zIndex("overlay"),
      toEqual([{ prop: "z-index", value: "50" }]),
    ),
    check(
      zIndex("content"),
      toEqual([{ prop: "z-index", value: "1" }]),
    ),
  ]));

test("typeStyle bundles size / leading / weight from the token", () =>
  all([
    check(
      typeStyle("h1"),
      toEqual([
        { prop: "font-size", value: "1.875rem" },
        { prop: "line-height", value: "1.25" },
        { prop: "font-weight", value: "400" },
      ]),
    ),
    check(
      typeStyle("body"),
      toEqual([
        { prop: "font-size", value: "1rem" },
        { prop: "line-height", value: "1.75" },
        { prop: "font-weight", value: "400" },
      ]),
    ),
  ]));

test("measure caps the reading width at the metric var", () =>
  check(
    measure,
    toEqual([
      {
        prop: "max-width",
        value: "var(--pm-measure)",
      },
    ]),
  ));
