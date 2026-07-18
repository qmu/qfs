import {
  type SoftStr,
  type Option,
  matchOption,
} from "plgg";
import {
  type Html,
  slot,
  input,
  attr,
  id_,
  name_,
  type_,
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
 * A CONTROLLED single-line text input. Its value comes
 * from props (never internal state), it is associated
 * with a real `<label>` by id, renders its error via
 * `aria-invalid` + `aria-describedby`, and — the recorded
 * interaction rule — a disabled input wears the native
 * attribute AND more-than-colour (a `pm-disabled` class:
 * dimmed + `not-allowed` cursor) AND withholds hover
 * feedback. `focusRing` on focus.
 */
export type TextInputProps<Msg> = Readonly<{
  name: SoftStr;
  label: SoftStr;
  value: SoftStr;
  placeholder: Option<SoftStr>;
  error: Option<SoftStr>;
  disabled: boolean;
  onInput: (value: SoftStr) => Msg;
}>;

export const textInput = <Msg>(
  props: TextInputProps<Msg>,
): Html<Msg, "div"> =>
  slot(
    [attr("class", `${cssPrefix}-field`)],
    [
      fieldLabel<Msg>(props.name, props.label),
      input(
        [
          id_(props.name),
          name_(props.name),
          type_("text"),
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
        [],
      ),
      ...fieldError<Msg>(props.name, props.error),
    ],
  );
