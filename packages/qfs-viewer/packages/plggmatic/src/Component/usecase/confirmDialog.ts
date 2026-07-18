import { type SoftStr } from "plgg";
import {
  type Html,
  slot,
  h2,
  p,
  button,
  text,
  attr,
  id_,
  key,
  onClick,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import { focusRing } from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";

/**
 * A modal confirmation for a destructive action — the
 * framework rendering the workbench oracle lacked the
 * a11y for. Backdrop and dialog are SIBLING keyed nodes
 * so an inside-click never bubbles to the backdrop's
 * cancel; the dialog carries `role="dialog"`,
 * `aria-modal="true"`, and `aria-labelledby` wired to its
 * title. A destructive confirm wears the `danger` role.
 * Backdrop-click and Cancel both dispatch `onCancel`;
 * confirm dispatches `onConfirm` — the component owns zero
 * state (the scheduler owns the pending-confirmation).
 */
export type ConfirmDialogProps<Msg> = Readonly<{
  title: SoftStr;
  body: SoftStr;
  confirmLabel: SoftStr;
  cancelLabel: SoftStr;
  destructive: boolean;
  onConfirm: Msg;
  onCancel: Msg;
}>;

const TITLE_ID = `${cssPrefix}-dialog-title`;

export const confirmDialog = <Msg>(
  props: ConfirmDialogProps<Msg>,
): Html<Msg, "div"> =>
  slot(
    [attr("class", `${cssPrefix}-modal`)],
    [
      slot(
        [
          key("backdrop"),
          attr("class", `${cssPrefix}-backdrop`),
          onClick(props.onCancel),
        ],
        [],
      ),
      slot(
        [
          key("dialog"),
          attr("class", `${cssPrefix}-dialog`),
          attr("role", "dialog"),
          attr("aria-modal", "true"),
          attr("aria-labelledby", TITLE_ID),
        ],
        [
          h2(
            [id_(TITLE_ID)],
            [text(props.title)],
          ),
          p([], [text(props.body)]),
          slot(
            [
              attr(
                "class",
                `${cssPrefix}-dialog-actions`,
              ),
            ],
            [
              button(
                [
                  style_(
                    `${cssPrefix}-btn`,
                    focusRing,
                  ),
                  onClick(props.onCancel),
                ],
                [text(props.cancelLabel)],
              ),
              button(
                [
                  style_(
                    props.destructive
                      ? `${cssPrefix}-btn ${cssPrefix}-btn-danger`
                      : `${cssPrefix}-btn`,
                    focusRing,
                  ),
                  onClick(props.onConfirm),
                ],
                [text(props.confirmLabel)],
              ),
            ],
          ),
        ],
      ),
    ],
  );
