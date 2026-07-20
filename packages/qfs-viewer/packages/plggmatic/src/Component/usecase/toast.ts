import { type SoftStr } from "plgg";
import {
  type Html,
  slot,
  span,
  button,
  text,
  attr,
  key,
  onClick,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import { focusRing } from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";

/**
 * A toast's tone — the semantic role set (D9). A closed
 * union so a renderer/CSS map is exhaustive; each tone is
 * styled through its role's `surface`/`text`/`border`
 * variants (`--pm-<tone>-*`).
 */
export type Tone =
  "success" | "danger" | "warning" | "info";

export const tones: ReadonlyArray<Tone> = [
  "success",
  "danger",
  "warning",
  "info",
];

/**
 * A single feedback toast. Announced via `role="status"`
 * with `aria-live` — `danger` escalates to `assertive`
 * (an error interrupts), the rest stay `polite`.
 * Dismissible via an `aria-label`led close that dispatches
 * `onDismiss`; the component owns NO timer — auto-dismiss
 * is the consumer's `cmdEffect`.
 */
export type ToastProps<Msg> = Readonly<{
  id: SoftStr;
  tone: Tone;
  message: SoftStr;
  onDismiss: Msg;
}>;

export const toast = <Msg>(
  props: ToastProps<Msg>,
): Html<Msg, "div"> =>
  slot(
    [
      key(props.id),
      attr(
        "class",
        `${cssPrefix}-toast ${cssPrefix}-toast-${props.tone}`,
      ),
      attr("role", "status"),
      attr(
        "aria-live",
        props.tone === "danger"
          ? "assertive"
          : "polite",
      ),
    ],
    [
      span([], [text(props.message)]),
      button(
        [
          attr("aria-label", "Dismiss"),
          style_(
            `${cssPrefix}-toast-close`,
            focusRing,
          ),
          onClick(props.onDismiss),
        ],
        [text("×")],
      ),
    ],
  );

/**
 * The keyed toaster stack — a live region holding the
 * current toasts (keyed by id so enter/exit motion plays
 * on the right node under the reduced-motion block).
 */
export const toaster = <Msg>(
  toasts: ReadonlyArray<ToastProps<Msg>>,
): Html<Msg, "div"> =>
  slot(
    [
      attr("class", `${cssPrefix}-toaster`),
      attr("aria-live", "polite"),
    ],
    toasts.map((t: ToastProps<Msg>) => toast(t)),
  );
