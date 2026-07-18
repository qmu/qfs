import { type SoftStr, matchOption } from "plgg";
import {
  type Html,
  type Flow,
  ul,
  li,
  a,
  span,
  text,
  attr,
  href,
} from "plgg-view";
import {
  style_,
  textColor,
  listNone,
  p,
  pl,
} from "plggmatic/styleEntry";
import {
  focusRing,
  hoverDim,
} from "plggmatic/Component/model/interaction";
import { type NavItem } from "plggmatic/Component/model/navItem";

/**
 * The navigation-tree component: a recursive,
 * config-driven tree (the docs-site sidebar). **Recorded
 * rule**: navTree emits LIST markup only — a `<ul>` of
 * `<li>`, nested for children — and does NOT wrap itself
 * in a `<nav>` landmark; the landmark is owned by the
 * Layout `navigation` pane it sits inside, so the two
 * never produce nested/duplicate nav landmarks (the
 * higher-level pane policy wins, per this ticket's
 * coordination note). The leaf whose `href` equals
 * `activePath` is marked `aria-current="page"` at build
 * time; a header node (`href` `None`) renders plain
 * text. Zero client JavaScript — the whole tree is
 * static SSR. Links carry the shared focus ring and
 * hover feedback.
 */
export const navTree = (
  items: ReadonlyArray<NavItem>,
  activePath: SoftStr,
): Html<never, "ul"> => {
  const label = (item: NavItem): Flow<never> =>
    matchOption<SoftStr, Flow<never>>(
      () =>
        span(
          [style_(textColor("muted"))],
          [text(item.label)],
        ),
      (to) =>
        a(
          [
            href(to),
            ...(to === activePath
              ? [attr("aria-current", "page")]
              : []),
            style_(
              textColor("text"),
              focusRing,
              hoverDim,
            ),
          ],
          [text(item.label)],
        ),
    )(item.href);

  const renderItem = (
    item: NavItem,
  ): Html<never, "li"> =>
    li(
      [],
      item.children.length === 0
        ? [label(item)]
        : [label(item), list(item.children)],
    );

  const list = (
    nodes: ReadonlyArray<NavItem>,
  ): Html<never, "ul"> =>
    ul(
      [style_(listNone, p(0), pl(3))],
      nodes.map(renderItem),
    );

  return list(items);
};
