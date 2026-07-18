import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { renderToString } from "plgg-view";
import { div, text } from "plgg-view";
import { formView } from "plggmatic/Form/usecase/formView";

const fields = [
  div([], [text("field one")]),
  div([], [text("field two")]),
];

test("the form wraps its fields and an enabled submit button", () => {
  const html = renderToString(
    formView({
      fields,
      submitLabel: "Save",
      submitting: false,
      onSubmit: { submit: true },
    }),
  );
  return all([
    check(html.startsWith("<form"), toBe(true)),
    check(html.includes("field one"), toBe(true)),
    check(
      html.includes('type="submit"'),
      toBe(true),
    ),
    check(
      html.includes(">Save</button>"),
      toBe(true),
    ),
    check(html.includes("disabled"), toBe(false)),
  ]);
});

test("a submitting form disables the submit button", () =>
  check(
    renderToString(
      formView({
        fields,
        submitLabel: "Save",
        submitting: true,
        onSubmit: { submit: true },
      }),
    ).includes("disabled"),
    toBe(true),
  ));
