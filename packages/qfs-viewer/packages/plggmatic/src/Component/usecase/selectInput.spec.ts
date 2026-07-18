import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { some, none } from "plgg";
import { renderToString } from "plgg-view";
import { selectInput } from "plggmatic/Component/usecase/selectInput";

const html = renderToString(
  selectInput({
    name: "role",
    label: "Role",
    value: "guest",
    options: [
      { value: "admin", label: "Admin" },
      { value: "guest", label: "Guest" },
    ],
    error: none(),
    disabled: false,
    onChange: (v) => ({ v }),
  }),
);

test("selectInput renders its options and marks the selected one", () =>
  all([
    check(html.includes("<select"), toBe(true)),
    check(
      html.includes(
        '<option value="admin">Admin</option>',
      ),
      toBe(true),
    ),
    check(
      html.includes(
        '<option value="guest" selected="">Guest</option>',
      ),
      toBe(true),
    ),
  ]));

test("selectInput surfaces its error and disables", () =>
  check(
    renderToString(
      selectInput({
        name: "role",
        label: "Role",
        value: "",
        options: [{ value: "a", label: "A" }],
        error: some("Pick one"),
        disabled: true,
        onChange: (v) => ({ v }),
      }),
    ).includes('aria-invalid="true"'),
    toBe(true),
  ));
