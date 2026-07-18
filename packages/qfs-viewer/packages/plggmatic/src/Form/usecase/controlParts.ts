import {
  type SoftStr,
  type Option,
  isSome,
  matchOption,
} from "plgg";
import {
  type Html,
  type Attribute,
  label,
  span,
  text,
  attr,
  for_,
  id_,
} from "plgg-view";
import { cssPrefix } from "plggmatic/Meta/model/identity";

/**
 * The shared accessibility + labelling pieces every form
 * control uses (ticket 12), so label association, the
 * `aria-invalid`/`aria-describedby` error wiring, and the
 * `danger`-role error text are framework-owned and
 * identical across `textInput`/`textArea`/`selectInput`/
 * `checkbox` — spec-asserted once here.
 */

/** The `<label for=name>` associated with a control by id. */
export const fieldLabel = <Msg>(
  name: SoftStr,
  labelText: SoftStr,
): Html<Msg, "label"> =>
  label(
    [
      for_(name),
      attr("class", `${cssPrefix}-field-label`),
    ],
    [text(labelText)],
  );

/**
 * The id a control's error text carries and its
 * `aria-describedby` points at.
 */
export const errorId = (name: SoftStr): SoftStr =>
  `${name}-error`;

/**
 * The `aria-invalid` + `aria-describedby` attributes when
 * the field has an error (empty otherwise) — spread into
 * the control's attribute list.
 */
export const errorAria = (
  name: SoftStr,
  error: Option<SoftStr>,
): ReadonlyArray<Attribute<never>> =>
  isSome(error)
    ? [
        attr("aria-invalid", "true"),
        attr("aria-describedby", errorId(name)),
      ]
    : [];

/**
 * The error text node (in the `danger` text role),
 * carrying the id `aria-describedby` references. Empty
 * when there is no error.
 */
export const fieldError = <Msg>(
  name: SoftStr,
  error: Option<SoftStr>,
): ReadonlyArray<Html<Msg>> =>
  matchOption<SoftStr, ReadonlyArray<Html<Msg>>>(
    () => [],
    (msg: SoftStr) => [
      span(
        [
          id_(errorId(name)),
          attr(
            "class",
            `${cssPrefix}-field-error`,
          ),
          attr("role", "alert"),
        ],
        [text(msg)],
      ),
    ],
  )(error);
