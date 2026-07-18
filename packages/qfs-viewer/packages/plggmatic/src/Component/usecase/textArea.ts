import {
  type SoftStr,
  type Option,
  matchOption,
} from "plgg";
import {
  type Html,
  slot,
  textarea,
  text,
  attr,
  id_,
  name_,
  value_,
  placeholder_,
  disabled_,
  onInput,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import {
  focusRing,
  hoverDim,
} from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";
import {
  fieldLabel,
  errorAria,
  fieldError,
} from "plggmatic/Form/usecase/controlParts";

/**
 * A CONTROLLED multi-line text control — `textInput`'s
 * sibling for prose drafts. Same recorded rules: value
 * from props, labelled by id, `aria-invalid`/
 * `aria-describedby` error wiring, disabled = native +
 * `pm-disabled` + withheld hover. The value rides both
 * the `value` attribute (the runtime's controlled sync)
 * and the text child (SSR's initial render).
 */
export type TextAreaProps<Msg> = Readonly<{
  name: SoftStr;
  label: SoftStr;
  value: SoftStr;
  placeholder: Option<SoftStr>;
  error: Option<SoftStr>;
  disabled: boolean;
  onInput: (value: SoftStr) => Msg;
}>;

export const textArea = <Msg>(
  props: TextAreaProps<Msg>,
): Html<Msg, "div"> =>
  slot(
    [attr("class", `${cssPrefix}-field`)],
    [
      fieldLabel<Msg>(props.name, props.label),
      textarea(
        [
          id_(props.name),
          name_(props.name),
          value_(props.value),
          ...matchOption<
            SoftStr,
            ReadonlyArray<
              ReturnType<typeof placeholder_>
            >
          >(
            () => [],
            (p: SoftStr) => [placeholder_(p)],
          )(props.placeholder),
          ...errorAria(props.name, props.error),
          ...disabled_(props.disabled),
          style_(
            `${cssPrefix}-input`,
            focusRing,
            ...(props.disabled
              ? [`${cssPrefix}-disabled`]
              : [hoverDim]),
          ),
          onInput(props.onInput),
        ],
        [text(props.value)],
      ),
      ...fieldError<Msg>(props.name, props.error),
    ],
  );
