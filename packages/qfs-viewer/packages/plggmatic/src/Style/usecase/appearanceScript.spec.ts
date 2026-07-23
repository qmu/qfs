import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type SchemeRoot,
  type SchemeStorage,
  appearanceInitScript,
  injectAppearanceScript,
  applyScheme,
} from "plggmatic/Style/usecase/appearanceScript";

// An in-memory root + storage so the DOM-side helper is
// driven without a browser (no `as`, no DOM env).
const fakes = () => {
  const classes = new Set<string>();
  const store = new Map<string, string>();
  const root: SchemeRoot = {
    classList: {
      add: (t) => {
        classes.add(t);
      },
      remove: (t) => {
        classes.delete(t);
      },
    },
  };
  const storage: SchemeStorage = {
    setItem: (k, v) => {
      store.set(k, v);
    },
  };
  return { classes, store, root, storage };
};

test("init script reads the key, queries prefers-color-scheme, targets documentElement, and has no </script", () =>
  all([
    check(
      appearanceInitScript.includes(
        "vp-appearance",
      ),
      toBe(true),
    ),
    check(
      appearanceInitScript.includes(
        "prefers-color-scheme: dark",
      ),
      toBe(true),
    ),
    check(
      appearanceInitScript.includes(
        "documentElement",
      ),
      toBe(true),
    ),
    check(
      appearanceInitScript.includes("</script"),
      toBe(false),
    ),
  ]));

test("injectAppearanceScript inserts before </head>, else passes through", () =>
  all([
    check(
      injectAppearanceScript(
        "<head><title>x</title></head>",
      ).includes("<script>"),
      toBe(true),
    ),
    check(
      injectAppearanceScript(
        "<head></head>",
      ).indexOf("<script>") <
        injectAppearanceScript(
          "<head></head>",
        ).indexOf("</head>"),
      toBe(true),
    ),
    check(
      injectAppearanceScript(
        "<body>no head</body>",
      ),
      toBe("<body>no head</body>"),
    ),
  ]));

test("applyScheme adds dark, persists, and swallows storage failures", () => {
  const f = fakes();
  applyScheme("dark", f.root, f.storage);
  const afterDark = f.classes.has("dark");
  applyScheme("light", f.root, f.storage);
  const afterLight = f.classes.has("dark");
  const throwing: SchemeStorage = {
    setItem: () => {
      throw new Error("blocked");
    },
  };
  // must not throw
  applyScheme("dark", f.root, throwing);
  return all([
    check(afterDark, toBe(true)),
    check(afterLight, toBe(false)),
    check(
      f.store.get("vp-appearance"),
      toBe("light"),
    ),
    check(f.classes.has("dark"), toBe(true)),
  ]);
});
