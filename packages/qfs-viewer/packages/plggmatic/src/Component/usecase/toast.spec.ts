import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import { renderToString } from "plgg-view";
import {
  tones,
  toast,
  toaster,
} from "plggmatic/Component/usecase/toast";

test("the tones are the semantic role set", () =>
  check(
    tones,
    toEqual([
      "success",
      "danger",
      "warning",
      "info",
    ]),
  ));

test("a success toast is a polite live status with a dismiss", () => {
  const html = renderToString(
    toast({
      id: "t1",
      tone: "success",
      message: "Saved",
      onDismiss: { dismissed: "t1" },
    }),
  );
  return all([
    check(
      html.includes('role="status"'),
      toBe(true),
    ),
    check(
      html.includes('aria-live="polite"'),
      toBe(true),
    ),
    check(
      html.includes("pm-toast-success"),
      toBe(true),
    ),
    check(
      html.includes('aria-label="Dismiss"'),
      toBe(true),
    ),
  ]);
});

test("a danger toast escalates to assertive", () =>
  check(
    renderToString(
      toast({
        id: "t2",
        tone: "danger",
        message: "Failed",
        onDismiss: { dismissed: "t2" },
      }),
    ).includes('aria-live="assertive"'),
    toBe(true),
  ));

test("the toaster is a live region holding its toasts", () => {
  const html = renderToString(
    toaster([
      {
        id: "a",
        tone: "info",
        message: "One",
        onDismiss: { dismissed: "a" },
      },
      {
        id: "b",
        tone: "warning",
        message: "Two",
        onDismiss: { dismissed: "b" },
      },
    ]),
  );
  return all([
    check(
      html.includes("pm-toaster"),
      toBe(true),
    ),
    check(html.includes("One"), toBe(true)),
    check(html.includes("Two"), toBe(true)),
  ]);
});
