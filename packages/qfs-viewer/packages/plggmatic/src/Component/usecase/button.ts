import { type SoftStr } from "plgg";
import {
  type Html,
  button as buttonEl,
  text,
  attr,
  type_,
  onClick,
} from "plgg-view";
import {
  style_,
  bg,
  textColor,
  px,
  py,
  rounded,
  weight,
  medium,
  pointer,
  decl,
} from "plggmatic/styleEntry";
import {
  focusRing,
  hoverDim,
  pressDim,
} from "plggmatic/Component/model/interaction";

/**
 * A button's props. `onPress` is the typed `Msg` the
 * click produces (components are pure views — no
 * internal state); `disabled` is a real boolean, not a
 * styled look-alike.
 */
export type ButtonProps<Msg> = Readonly<{
  label: SoftStr;
  onPress: Msg;
  disabled: boolean;
}>;

/**
 * The action component. **Recorded rule**: an action is
 * always a real `<button type="button">` (never a
 * div-with-onclick), and `disabled` is conveyed by MORE
 * than color — the native `disabled` attribute (which
 * also drops it from the tab order) plus a
 * `cursor:not-allowed` and reduced opacity, and the
 * hover/press feedback is withheld. Enabled buttons
 * carry the shared {@link focusRing}/{@link
 * hoverDim}/{@link pressDim} state set. Styled only
 * through token utilities.
 */
export const button = <Msg>(
  props: ButtonProps<Msg>,
): Html<Msg, "button"> =>
  buttonEl(
    [
      type_("button"),
      ...(props.disabled
        ? [attr("disabled", "")]
        : [onClick(props.onPress)]),
      style_(
        bg("primary-base"),
        textColor("surface"),
        px(4),
        py(2),
        rounded("md"),
        weight(medium),
        focusRing,
        ...(props.disabled
          ? [
              decl("cursor", "not-allowed"),
              decl("opacity", "0.6"),
            ]
          : [pointer, hoverDim, pressDim]),
      ),
    ],
    [text(props.label)],
  );
