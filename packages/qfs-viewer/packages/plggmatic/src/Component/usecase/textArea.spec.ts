import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { some, none } from "plgg";
import { renderToString } from "plgg-view";
import { textArea } from "plggmatic/Component/usecase/textArea";

const html = renderToString(
  textArea({
    name: "body",
    label: "Body",
    value: "draft text",
    placeholder: some("Write…"),
    error: none(),
    disabled: false,
    onInput: (v) => ({ v }),
  }),
);

test("textArea is a labelled controlled textarea", () =>
  all([
    check(
      html.includes('<label for="body"'),
      toBe(true),
    ),
    check(html.includes("<textarea"), toBe(true)),
    check(
      html.includes(">draft text</textarea>"),
      toBe(true),
    ),
    check(
      html.includes('value="draft text"'),
      toBe(true),
    ),
  ]));

test("textArea surfaces its error", () =>
  check(
    renderToString(
      textArea({
        name: "body",
        label: "Body",
        value: "",
        placeholder: none(),
        error: some("Required"),
        disabled: false,
        onInput: (v) => ({ v }),
      }),
    ).includes('aria-describedby="body-error"'),
    toBe(true),
  ));
