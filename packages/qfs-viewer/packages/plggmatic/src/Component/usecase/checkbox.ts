import { type SoftStr } from "plgg";
import {
  type Html,
  slot,
  label,
  input,
  text,
  attr,
  id_,
  name_,
  type_,
  for_,
  checked_,
  disabled_,
  onChange,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import { focusRing } from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";

/**
 * A CONTROLLED checkbox. `checked` comes from props (the
 * runtime reflects it onto the live `.checked` property);
 * the recorded interaction rule — it dispatches on
 * `onChange` IGNORING the event payload (the state is the
 * model's `checked`, not the DOM's), the oracle's
 * pattern. Labelled by id, `focusRing` on focus, disabled
 * = native attribute + `pm-disabled`.
 */
export type CheckboxProps<Msg> = Readonly<{
  name: SoftStr;
  label: SoftStr;
  checked: boolean;
  disabled: boolean;
  onToggle: Msg;
}>;

export const checkbox = <Msg>(
  props: CheckboxProps<Msg>,
): Html<Msg, "div"> =>
  slot(
    [
      attr(
        "class",
        `${cssPrefix}-field ${cssPrefix}-check`,
      ),
    ],
    [
      input(
        [
          id_(props.name),
          name_(props.name),
          type_("checkbox"),
          ...checked_(props.checked),
          ...disabled_(props.disabled),
          style_(
            `${cssPrefix}-checkbox`,
            focusRing,
            ...(props.disabled
              ? [`${cssPrefix}-disabled`]
              : []),
          ),
          onChange(() => props.onToggle),
        ],
        [],
      ),
      label(
        [
          for_(props.name),
          attr(
            "class",
            `${cssPrefix}-check-label`,
          ),
        ],
        [text(props.label)],
      ),
    ],
  );
