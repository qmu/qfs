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
} from "plgg-view";
import {
  basis,
  fluid,
  p,
} from "plggmatic/styleEntry";
import {
  row,
  column,
  navPane,
  mainPane,
  asidePane,
} from "plggmatic/Layout/usecase/combinators";

// The compositional pattern in one expression: a fixed
// nav track, a fluid reading track — geometry composed
// as atoms, structure as nesting, no config object.
const strip = row(
  [],
  [
    column(
      [basis("220px")],
      [navPane([p(2)], [text("nav")])],
    ),
    column(
      [fluid],
      [
        mainPane([p(4)], [text("read")]),
        asidePane([], [text("meta")]),
      ],
    ),
  ],
);

const rendered = renderToString(strip);
const css = collectCss(strip);

test("panes render as real landmarks", () =>
  all([
    check(rendered.includes("<nav"), toBe(true)),
    check(rendered.includes("<main"), toBe(true)),
    check(
      rendered.includes("<aside"),
      toBe(true),
    ),
  ]));

test("combinators expose their class hooks", () =>
  all([
    check(
      rendered.includes('class="pm-row'),
      toBe(true),
    ),
    check(
      rendered.includes('class="pm-col'),
      toBe(true),
    ),
    check(
      rendered.includes('class="pm-pane'),
      toBe(true),
    ),
  ]));

test("consumer parts merge into the one class list", () =>
  all([
    // sizing atoms land as atomic rules...
    check(
      css.includes("flex:0 0 220px"),
      toBe(true),
    ),
    check(
      css.includes("flex:1 1 auto"),
      toBe(true),
    ),
    check(
      css.includes("min-width:0"),
      toBe(true),
    ),
    // ...and the base flow is still present
    check(
      css.includes("display:flex"),
      toBe(true),
    ),
    check(
      css.includes("flex-direction:column"),
      toBe(true),
    ),
  ]));

test("a custom hook rides the same class list", () => {
  const hooked = renderToString(
    column(["ex-reader", fluid], [text("x")]),
  );
  return all([
    check(
      hooked.includes('class="pm-col'),
      toBe(true),
    ),
    // one class attribute carries hook + atoms — no
    // second class attr to clobber it
    check(
      hooked.includes(" ex-reader"),
      toBe(true),
    ),
    check(
      hooked.split('class="').length - 1,
      toBe(1),
    ),
  ]);
});

test("combinators are pure", () =>
  all([
    check(renderToString(strip), toBe(rendered)),
    check(collectCss(strip), toBe(css)),
  ]));
