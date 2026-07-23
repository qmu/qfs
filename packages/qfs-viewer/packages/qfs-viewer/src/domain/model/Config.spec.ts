import {
  test,
  check,
  all,
  toBe,
  toEqual,
  shouldBeOk,
  shouldBeErr,
  andThen,
} from "plgg-test";
import {
  asConfig,
  defaultConfig,
} from "#qfs-viewer/domain/model/Config";

test("the default config has no opinions", () =>
  all([
    check(
      defaultConfig.title.__tag,
      toBe("None"),
    ),
    check(defaultConfig.tagGroups, toEqual([])),
    check(defaultConfig.hide.__tag, toBe("None")),
  ]));

test("an empty object is a valid config", () =>
  andThen(shouldBeOk()(asConfig({})), (c) =>
    all([
      check(c.title.__tag, toBe("None")),
      check(c.tagGroups, toEqual([])),
    ]),
  ));

test("a title is read", () =>
  andThen(
    shouldBeOk()(asConfig({ title: "Ours" })),
    (c) =>
      check(
        c.title.__tag === "Some" &&
          c.title.content,
        toBe("Ours"),
      ),
  ));

test("a tag group's key, label and variations are read", () =>
  andThen(
    shouldBeOk()(
      asConfig({
        tagGroups: [
          {
            key: "type",
            label: "Kind",
            variations: ["bugfix", "refactor"],
          },
        ],
      }),
    ),
    (c) =>
      all([
        check(c.tagGroups.length, toBe(1)),
        check(c.tagGroups[0]?.key, toBe("type")),
        check(
          c.tagGroups[0]?.label.__tag ===
            "Some" &&
            c.tagGroups[0]?.label.content,
          toBe("Kind"),
        ),
        check(
          c.tagGroups[0]?.variations.__tag ===
            "Some" &&
            c.tagGroups[0]?.variations.content,
          toEqual(["bugfix", "refactor"]),
        ),
      ]),
  ));

test("a tag group may declare only a key", () =>
  andThen(
    shouldBeOk()(
      asConfig({ tagGroups: [{ key: "layer" }] }),
    ),
    (c) =>
      all([
        check(
          c.tagGroups[0]?.label.__tag,
          toBe("None"),
        ),
        check(
          c.tagGroups[0]?.variations.__tag,
          toBe("None"),
        ),
      ]),
  ));

// A config is a thing a person wrote on purpose, so a typo is a question they
// want answered — not a field to quietly drop. Same rule as parseListQuery.
test("a malformed config is rejected, never repaired", () =>
  all([
    check(
      asConfig("not an object"),
      shouldBeErr(),
    ),
    check(asConfig([1, 2]), shouldBeErr()),
    check(asConfig(null), shouldBeErr()),
    check(asConfig({ title: 42 }), shouldBeErr()),
    check(
      asConfig({ tagGroups: "type" }),
      shouldBeErr(),
    ),
    check(
      asConfig({
        tagGroups: [{ label: "no key" }],
      }),
      shouldBeErr(),
    ),
    check(
      asConfig({ tagGroups: [{ key: "" }] }),
      shouldBeErr(),
    ),
    check(
      asConfig({
        tagGroups: [{ key: "x", label: 1 }],
      }),
      shouldBeErr(),
    ),
    check(
      asConfig({
        tagGroups: [
          { key: "x", variations: "a" },
        ],
      }),
      shouldBeErr(),
    ),
    check(
      asConfig({
        tagGroups: [
          { key: "x", variations: [1] },
        ],
      }),
      shouldBeErr(),
    ),
    check(
      asConfig({ hide: "author" }),
      shouldBeErr(),
    ),
    check(asConfig({ hide: [1] }), shouldBeErr()),
  ]));

// The message has to name the field, or a person cannot fix it without
// bisecting their own config.
test("a rejection names the field that is wrong", () =>
  andThen(
    shouldBeErr()(
      asConfig({
        tagGroups: [
          { key: "ok" },
          { key: "x", variations: [1] },
        ],
      }),
    ),
    (e) =>
      toBe(true)(
        e.content.message.includes(
          "tagGroups[1].variations",
        ),
      ),
  ));

// A config format grows. A repository that pins an older qfs-viewer should
// not fail to start because it mentions a field the older build never learned.
test("an unknown top-level key is allowed through in silence", () =>
  check(
    asConfig({
      title: "Ours",
      somethingFromTheFuture: { deep: true },
    }),
    shouldBeOk(),
  ));

test("hide is read as a list of keys", () =>
  andThen(
    shouldBeOk()(
      asConfig({
        hide: ["author", "created_at"],
      }),
    ),
    (c) =>
      check(
        c.hide.__tag === "Some" && c.hide.content,
        toEqual(["author", "created_at"]),
      ),
  ));

// ---- the qfs connection, chosen by configuration ----

// The zero-config default is the plan's issuance form ② — spawn the qfs on
// PATH per query — because `npx qfs-viewer` must work with no daemon running.
test("a config with no qfs key gets the on-demand spawn connection", () =>
  andThen(shouldBeOk()(asConfig({})), (c) =>
    check(
      c.qfs,
      toEqual({ __tag: "Spawn", bin: "qfs" }),
    ),
  ));

test("the qfs connection form is swappable by configuration", () =>
  andThen(
    shouldBeOk()(
      asConfig({
        qfs: {
          form: "remote",
          url: "https://qfs.example.com",
        },
      }),
    ),
    (c) =>
      check(
        c.qfs,
        toEqual({
          __tag: "Remote",
          url: "https://qfs.example.com",
        }),
      ),
  ));

// A malformed connection stops the boot like every other config error —
// falling back to spawn would run a binary the author asked not to run.
test("a malformed qfs key is rejected, not defaulted", () =>
  andThen(
    shouldBeErr()(
      asConfig({ qfs: { form: "daemon" } }),
    ),
    (e) =>
      check(
        e.content.message.includes("qfs.form"),
        toBe(true),
      ),
  ));

// ---- the collection switch (docs/adr/0008) ----

// Absent means the legacy scan serves — the pre-collection behaviour, and
// the zero-config default until the recorded retirement date.
test("a config with no collection key stays on the legacy scan", () =>
  andThen(shouldBeOk()(asConfig({})), (c) =>
    check(c.collection.__tag, toBe("None")),
  ));

test("a declared collection names the qfs markdown tree", () =>
  andThen(
    shouldBeOk()(
      asConfig({ collection: "strategy" }),
    ),
    (c) =>
      check(
        c.collection.__tag === "Some"
          ? c.collection.content
          : "",
        toBe("strategy"),
      ),
  ));

// The name is embedded verbatim into a statement, so a value the charset
// refuses stops the boot — falling back to the scanner would silently serve
// a different corpus than the author declared.
test("a malformed collection is rejected, not defaulted", () =>
  andThen(
    shouldBeErr()(
      asConfig({ collection: "a b|>c" }),
    ),
    (e) =>
      check(
        e.content.message.includes("collection"),
        toBe(true),
      ),
  ));
