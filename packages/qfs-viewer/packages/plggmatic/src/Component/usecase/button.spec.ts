import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  renderToString,
  collectCss,
} from "plgg-view";
import { button } from "plggmatic/Component/usecase/button";

const enabled = button({
  label: "Save",
  onPress: "save",
  disabled: false,
});
const disabled = button({
  label: "Save",
  onPress: "save",
  disabled: true,
});

test("button is a real <button type=button>", () =>
  all([
    check(
      renderToString(enabled).startsWith(
        "<button",
      ),
      toBe(true),
    ),
    check(
      renderToString(enabled).includes(
        'type="button"',
      ),
      toBe(true),
    ),
  ]));

test("disabled uses the native attribute", () =>
  all([
    check(
      renderToString(disabled).includes(
        "disabled",
      ),
      toBe(true),
    ),
    check(
      renderToString(enabled).includes(
        "disabled",
      ),
      toBe(false),
    ),
  ]));

test("focus + disabled differ by a non-color declaration", () =>
  all([
    // focus adds a geometric outline the default lacks
    check(
      collectCss(enabled).includes(
        ":focus-visible{outline:2px solid var(--pm-primary-base)",
      ),
      toBe(true),
    ),
    // disabled swaps the cursor (non-color) vs the
    // enabled pointer
    check(
      collectCss(enabled).includes(
        "cursor:pointer",
      ),
      toBe(true),
    ),
    check(
      collectCss(disabled).includes(
        "cursor:not-allowed",
      ),
      toBe(true),
    ),
  ]));

test("button is pure", () =>
  check(
    renderToString(
      button({
        label: "Save",
        onPress: "save",
        disabled: false,
      }),
    ),
    toBe(renderToString(enabled)),
  ));
