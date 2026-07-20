import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { some, none } from "plgg";
import { renderToString } from "plgg-view";
import { type NavItem } from "plggmatic/Component/model/navItem";
import { navTree } from "plggmatic/Component/usecase/navTree";

// A group header (no link) over two leaves; one of them
// is the active page.
const items: ReadonlyArray<NavItem> = [
  {
    label: "Guide",
    href: none(),
    children: [
      {
        label: "Intro",
        href: some("/guide/intro"),
        children: [],
      },
      {
        label: "Tokens",
        href: some("/guide/tokens"),
        children: [],
      },
    ],
  },
];

const rendered = renderToString(
  navTree(items, "/guide/tokens"),
);

test("navTree is list markup, not a nested landmark", () =>
  all([
    // list-rooted, so the Layout nav pane owns the
    // landmark (no nested <nav>)
    check(rendered.startsWith("<ul"), toBe(true)),
    check(rendered.includes("<nav"), toBe(false)),
    check(rendered.includes("<li"), toBe(true)),
  ]));

test("the active leaf is marked at build time", () =>
  all([
    check(
      rendered.includes(
        'href="/guide/tokens" aria-current="page"',
      ),
      toBe(true),
    ),
    // the non-active leaf carries no aria-current
    check(
      rendered.includes(
        'href="/guide/intro" aria-current',
      ),
      toBe(false),
    ),
  ]));

test("a link-less header renders as plain text, not a link", () =>
  all([
    check(rendered.includes("<span"), toBe(true)),
    check(
      rendered.includes(">Guide<"),
      toBe(true),
    ),
  ]));

test("navTree is pure", () =>
  check(
    renderToString(
      navTree(items, "/guide/tokens"),
    ),
    toBe(rendered),
  ));
