import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { some, none } from "plgg";
import { renderToString } from "plgg-view";
import {
  type Crumb,
  breadcrumb,
} from "plggmatic/Component/usecase/breadcrumb";

const crumbs: ReadonlyArray<Crumb> = [
  { label: "Sections", to: some("/app") },
  {
    label: "Botany",
    to: some("/app?c=sections"),
  },
  { label: "Moss", to: none() },
];

const rendered = renderToString(
  breadcrumb(crumbs),
);

test("the region is labelled for assistive tech", () =>
  check(
    rendered.includes('aria-label="Breadcrumb"'),
    toBe(true),
  ));

test("every crumb but the last is a truncating link", () =>
  all([
    check(
      rendered.includes('href="/app?c=sections"'),
      toBe(true),
    ),
    check(
      rendered.includes("pm-crumb-link"),
      toBe(true),
    ),
  ]));

test("the last crumb is emphasized plain text, not a link", () =>
  all([
    check(
      rendered.includes("pm-crumb-here"),
      toBe(true),
    ),
    check(
      rendered.includes(">Moss<"),
      toBe(true),
    ),
  ]));

test("separators are aria-hidden", () =>
  check(
    rendered.includes('aria-hidden="true"'),
    toBe(true),
  ));

test("a crumb with no target renders plain (mid-trail None)", () =>
  check(
    renderToString(
      breadcrumb([
        { label: "A", to: none() },
        { label: "B", to: some("/b") },
      ]),
    ).includes(">A<"),
    toBe(true),
  ));
