import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  renderToString,
  collectCss,
} from "plgg-view";
import { textLink } from "plggmatic/Component/usecase/textLink";

const internal = textLink({
  label: "Docs",
  to: "/docs",
  external: false,
});
const external = textLink({
  label: "GitHub",
  to: "https://github.com/qmu/plgg",
  external: true,
});

test("textLink is a real <a> with an href", () =>
  all([
    check(
      renderToString(internal).startsWith("<a"),
      toBe(true),
    ),
    check(
      renderToString(internal).includes(
        'href="/docs"',
      ),
      toBe(true),
    ),
  ]));

test("external link opens safely and announces itself", () =>
  all([
    check(
      renderToString(external).includes(
        'target="_blank"',
      ),
      toBe(true),
    ),
    check(
      renderToString(external).includes(
        'rel="noopener noreferrer"',
      ),
      toBe(true),
    ),
    check(
      renderToString(external).includes(
        "opens in a new tab",
      ),
      toBe(true),
    ),
    // internal links carry none of the external machinery
    check(
      renderToString(internal).includes("target"),
      toBe(false),
    ),
  ]));

test("link is identifiable by more than color (underline)", () =>
  check(
    collectCss(internal).includes(
      "text-decoration:underline",
    ),
    toBe(true),
  ));

test("textLink is pure", () =>
  check(
    renderToString(
      textLink({
        label: "Docs",
        to: "/docs",
        external: false,
      }),
    ),
    toBe(renderToString(internal)),
  ));
