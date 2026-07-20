import { type SoftStr } from "plgg";
import {
  type Html,
  type Flow,
  form,
  button,
  text,
  attr,
  type_,
  disabled_,
  onSubmit,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import {
  focusRing,
  hoverDim,
} from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";

/**
 * The form assembly: a `<form>` (plgg-view already
 * `preventDefault`s `onSubmit`) wrapping the field
 * controls and a submit button. When `submitting`, the
 * submit button is natively disabled and dims (the
 * pending state; the CONTROLS are disabled by the
 * consumer passing `disabled` to each). One `onSubmit`
 * Msg — the consumer's `update` parses the drafts
 * (`parseForm`) and folds to field errors or the Action's
 * `Cmd`; this component owns no state.
 */
export type FormViewProps<Msg> = Readonly<{
  fields: ReadonlyArray<Flow<Msg>>;
  submitLabel: SoftStr;
  submitting: boolean;
  onSubmit: Msg;
}>;

export const formView = <Msg>(
  props: FormViewProps<Msg>,
): Html<Msg, "form"> =>
  form(
    [
      onSubmit(props.onSubmit),
      attr("class", `${cssPrefix}-form`),
    ],
    [
      ...props.fields,
      button(
        [
          type_("submit"),
          ...disabled_(props.submitting),
          style_(
            `${cssPrefix}-btn ${cssPrefix}-btn-primary`,
            focusRing,
            ...(props.submitting
              ? [`${cssPrefix}-disabled`]
              : [hoverDim]),
          ),
        ],
        [
          text(
            props.submitting
              ? "Submitting…"
              : props.submitLabel,
          ),
        ],
      ),
    ],
  );
