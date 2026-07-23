import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { renderToString } from "plgg-view";
import { confirmDialog } from "plggmatic/Component/usecase/confirmDialog";

const html = renderToString(
  confirmDialog({
    title: "Delete note?",
    body: "This cannot be undone.",
    confirmLabel: "Delete",
    cancelLabel: "Cancel",
    destructive: true,
    onConfirm: { confirmed: true },
    onCancel: { cancelled: true },
  }),
);

test("the dialog carries modal a11y wired to its title", () =>
  all([
    check(
      html.includes('role="dialog"'),
      toBe(true),
    ),
    check(
      html.includes('aria-modal="true"'),
      toBe(true),
    ),
    check(
      html.includes(
        'aria-labelledby="pm-dialog-title"',
      ),
      toBe(true),
    ),
    check(
      html.includes('id="pm-dialog-title"'),
      toBe(true),
    ),
  ]));

test("backdrop and dialog are siblings; destructive confirm wears danger", () =>
  all([
    check(
      html.includes("pm-backdrop"),
      toBe(true),
    ),
    check(
      html.includes("pm-btn-danger"),
      toBe(true),
    ),
    check(html.includes(">Delete<"), toBe(true)),
    check(html.includes(">Cancel<"), toBe(true)),
  ]));
