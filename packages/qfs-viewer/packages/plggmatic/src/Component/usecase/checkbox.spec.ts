import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { renderToString } from "plgg-view";
import { checkbox } from "plggmatic/Component/usecase/checkbox";

test("a checked checkbox carries the checked attribute and a label", () => {
  const html = renderToString(
    checkbox({
      name: "agree",
      label: "I agree",
      checked: true,
      disabled: false,
      onToggle: { toggled: true },
    }),
  );
  return all([
    check(
      html.includes('type="checkbox"'),
      toBe(true),
    ),
    check(html.includes("checked"), toBe(true)),
    check(
      html.includes('<label for="agree"'),
      toBe(true),
    ),
    check(
      html.includes(">I agree</label>"),
      toBe(true),
    ),
  ]);
});

test("an unchecked disabled checkbox omits checked, adds disabled", () => {
  const html = renderToString(
    checkbox({
      name: "agree",
      label: "I agree",
      checked: false,
      disabled: true,
      onToggle: { toggled: true },
    }),
  );
  return all([
    check(html.includes("checked"), toBe(false)),
    check(html.includes("disabled"), toBe(true)),
  ]);
});
