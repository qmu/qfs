import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { some, none } from "plgg";
import { renderToString } from "plgg-view";
import { textInput } from "plggmatic/Component/usecase/textInput";

const clean = renderToString(
  textInput({
    name: "email",
    label: "Email",
    value: "a@b.c",
    placeholder: some("you@example.com"),
    error: none(),
    disabled: false,
    onInput: (v) => ({ v }),
  }),
);
const invalid = renderToString(
  textInput({
    name: "email",
    label: "Email",
    value: "nope",
    placeholder: none(),
    error: some("Not an email"),
    disabled: true,
    onInput: (v) => ({ v }),
  }),
);

test("a clean input is labelled, controlled, and not invalid", () =>
  all([
    check(
      clean.includes('<label for="email"'),
      toBe(true),
    ),
    check(
      clean.includes('id="email"'),
      toBe(true),
    ),
    check(
      clean.includes('value="a@b.c"'),
      toBe(true),
    ),
    check(
      clean.includes(
        'placeholder="you@example.com"',
      ),
      toBe(true),
    ),
    check(
      clean.includes("aria-invalid"),
      toBe(false),
    ),
  ]));

test("an invalid input wires aria + renders the error, and disables", () =>
  all([
    check(
      invalid.includes('aria-invalid="true"'),
      toBe(true),
    ),
    check(
      invalid.includes(
        'aria-describedby="email-error"',
      ),
      toBe(true),
    ),
    check(
      invalid.includes('id="email-error"'),
      toBe(true),
    ),
    check(
      invalid.includes("Not an email"),
      toBe(true),
    ),
    check(
      invalid.includes("disabled"),
      toBe(true),
    ),
  ]));
