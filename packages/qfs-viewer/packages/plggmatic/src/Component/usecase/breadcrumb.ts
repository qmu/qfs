import {
  type SoftStr,
  type Option,
  matchOption,
} from "plgg";
import {
  type Html,
  slot,
  span,
  a,
  text,
  attr,
  href,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import { focusRing } from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";

/**
 * One entry of a {@link breadcrumb} trail: a label and an
 * `Option` link target. A `Some` renders a link to the
 * URL that truncates the flow there (navigation, not a
 * mode switch — the workbench's "every crumb but the last
 * links to the URL that truncates the stack"); a `None`
 * renders plain text (a crumb with no shallower address).
 */
export type Crumb = Readonly<{
  label: SoftStr;
  to: Option<SoftStr>;
}>;

/**
 * The breadcrumb trail — the framework version of the
 * workbench's `breadcrumb`: one crumb per flow position,
 * every crumb but the LAST a link to its truncating URL,
 * the last emphasized plain text, `aria-hidden`
 * separators, all inside a labelled region so assistive
 * tech announces "Breadcrumb". Mirrors the same stack the
 * multi-column renderer walks, so the trail and the
 * columns can never disagree.
 */
export const breadcrumb = <Msg>(
  crumbs: ReadonlyArray<Crumb>,
): Html<Msg, "div"> => {
  const last = crumbs.length - 1;
  return slot(
    [
      attr("class", `${cssPrefix}-crumbs`),
      attr("aria-label", "Breadcrumb"),
    ],
    crumbs.flatMap(
      (
        crumb: Crumb,
        i: number,
      ): ReadonlyArray<Html<Msg>> => {
        const label =
          i === last
            ? span(
                [
                  attr(
                    "class",
                    `${cssPrefix}-crumb-here`,
                  ),
                ],
                [text(crumb.label)],
              )
            : matchOption<SoftStr, Html<Msg>>(
                () =>
                  span([], [text(crumb.label)]),
                (to: SoftStr) =>
                  a(
                    [
                      href(to),
                      style_(
                        `${cssPrefix}-crumb-link`,
                        focusRing,
                      ),
                    ],
                    [text(crumb.label)],
                  ),
              )(crumb.to);
        return i === 0
          ? [label]
          : [
              span(
                [
                  attr(
                    "class",
                    `${cssPrefix}-crumb-sep`,
                  ),
                  attr("aria-hidden", "true"),
                ],
                [text("›")],
              ),
              label,
            ];
      },
    ),
  );
};
