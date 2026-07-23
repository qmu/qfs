import { type SoftStr } from "plgg";
import {
  type Html,
  a,
  text,
  attr,
  href,
} from "plgg-view";
import {
  style_,
  textColor,
  decl,
} from "plggmatic/styleEntry";
import {
  focusRing,
  hoverDim,
} from "plggmatic/Component/model/interaction";

/**
 * A text link's props. `external` marks a link that
 * leaves the site, so it opens in a new tab AND is
 * announced as such (no surprise navigation — a
 * no-dark-patterns default).
 */
export type TextLinkProps = Readonly<{
  label: SoftStr;
  to: SoftStr;
  external: boolean;
}>;

/**
 * The navigation component. **Recorded rule**:
 * navigation is always a real `<a>` carrying an `href`
 * (never a div-with-onclick), with a standing underline
 * so a link is identifiable without relying on color
 * alone; an `external` link opens in a new tab and
 * announces it (`target=_blank` + `rel=noopener
 * noreferrer` + a visible `↗` affordance and an extended
 * accessible label), so the user is never surprised.
 * Carries the shared focus ring and hover feedback.
 */
export const textLink = (
  props: TextLinkProps,
): Html<never, "a"> =>
  a(
    [
      href(props.to),
      ...(props.external
        ? [
            attr("target", "_blank"),
            attr("rel", "noopener noreferrer"),
            attr(
              "aria-label",
              `${props.label} (opens in a new tab)`,
            ),
          ]
        : []),
      style_(
        textColor("primary-text"),
        decl("text-decoration", "underline"),
        focusRing,
        hoverDim,
      ),
    ],
    [
      text(
        props.external
          ? `${props.label} ↗`
          : props.label,
      ),
    ],
  );
