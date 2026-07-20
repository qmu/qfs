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
import {
  themeToggle,
  staticThemeToggle,
  themeToggleClass,
  themeToggleCss,
} from "plggmatic/Component/usecase/themeToggle";

const light = themeToggle({
  scheme: "light",
  toggle: "toggle",
});
const dark = themeToggle({
  scheme: "dark",
  toggle: "toggle",
});

test("themeToggle is a labelled <button>", () =>
  all([
    check(
      renderToString(light).startsWith("<button"),
      toBe(true),
    ),
    check(
      renderToString(light).includes(
        'aria-label="Switch to dark mode"',
      ),
      toBe(true),
    ),
    check(
      renderToString(dark).includes(
        'aria-label="Switch to light mode"',
      ),
      toBe(true),
    ),
  ]));

test("shows the current scheme's icon (shape, not color)", () =>
  all([
    // light shows the sun (8-ray path), dark the crescent
    check(
      renderToString(light).includes(
        "M12 18a6 6",
      ),
      toBe(true),
    ),
    check(
      renderToString(light).includes(
        "M9.822 2.238",
      ),
      toBe(false),
    ),
    check(
      renderToString(dark).includes(
        "M9.822 2.238",
      ),
      toBe(true),
    ),
  ]));

test("themeToggle carries the shared focus ring", () =>
  check(
    collectCss(light).includes(
      ":focus-visible{outline:2px solid var(--pm-primary-base)",
    ),
    toBe(true),
  ));

test("themeToggle is pure", () =>
  check(
    renderToString(
      themeToggle({
        scheme: "light",
        toggle: "toggle",
      }),
    ),
    toBe(renderToString(light)),
  ));

const staticHtml = renderToString(
  staticThemeToggle,
);

test("staticThemeToggle is a labelled <button> on the class hook", () =>
  all([
    check(
      staticHtml.startsWith("<button"),
      toBe(true),
    ),
    check(
      staticHtml.includes(
        'aria-label="Toggle dark mode"',
      ),
      toBe(true),
    ),
    check(
      staticHtml.includes(
        `class="${themeToggleClass}"`,
      ),
      toBe(true),
    ),
  ]));

test("staticThemeToggle renders BOTH icons (CSS, not build-time scheme, picks)", () =>
  all([
    // sun (8-ray) AND crescent both present
    check(
      staticHtml.includes("M12 18a6 6"),
      toBe(true),
    ),
    check(
      staticHtml.includes("M9.822 2.238"),
      toBe(true),
    ),
    // each on its own switch class
    check(
      staticHtml.includes('class="pm-sun"'),
      toBe(true),
    ),
    check(
      staticHtml.includes('class="pm-moon"'),
      toBe(true),
    ),
  ]));

test("staticThemeToggle emits no handler (SSG: the host body script wires it)", () =>
  // renderToString drops handlers; the static toggle
  // never had one, so its markup is inert by design.
  check(
    staticHtml.includes("onclick"),
    toBe(false),
  ));

test("themeToggleCss is token-driven, scheme-switched, and escape-safe", () =>
  all([
    // chrome on the neutral surface, primary hover edge
    check(
      themeToggleCss.includes(
        "background:var(--pm-surface)",
      ),
      toBe(true),
    ),
    check(
      themeToggleCss.includes(
        "border-color:var(--pm-primary-base)",
      ),
      toBe(true),
    ),
    // shared 2px focus ring (matches the runtime toggle)
    check(
      themeToggleCss.includes(
        "outline:2px solid var(--pm-primary-base)",
      ),
      toBe(true),
    ),
    // the crescent hides in light, the sun under dark
    check(
      themeToggleCss.includes(
        `.${themeToggleClass} .pm-moon{display:none}`,
      ),
      toBe(true),
    ),
    check(
      themeToggleCss.includes(
        `html.dark .${themeToggleClass} .pm-sun{display:none}`,
      ),
      toBe(true),
    ),
    // escape-safe: survives an SSR text escaper verbatim
    check(
      themeToggleCss.includes("<"),
      toBe(false),
    ),
    check(
      themeToggleCss.includes(">"),
      toBe(false),
    ),
    check(
      themeToggleCss.includes("&"),
      toBe(false),
    ),
  ]));
