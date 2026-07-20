import {
  test,
  all,
  toBe,
  toEqual,
  fail,
  type Assertion,
} from "plgg-test";
import {
  some,
  none,
  match,
  type SoftStr,
  type Option,
} from "plgg";
import {
  crumbsOf,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
  type Level,
  type RowLink,
} from "plggmatic";
import {
  rootLevel,
  docLevel,
  resourceLevel,
  qfsLevel,
  qfsErrorLevel,
  sceneOf,
} from "#qfs-viewer/domain/usecase/scene";
import { type DefaultView } from "#qfs-viewer/domain/model/Describe";

const view: DefaultView = {
  path: "/local/docs",
  archetype: "blob_namespace",
  columns: [
    { name: "path", type: "Text" },
    { name: "size", type: "Int" },
  ],
  rows: [
    {
      cells: ["/local/docs/a.md", "10"],
      child: some("/local/docs/a.md"),
    },
    // a row with NO contained child: display data, not
    // navigation — it must not become a Scene row.
    { cells: ["total", "12"], child: none() },
    // an empty first cell: the child path names the row.
    {
      cells: ["", "2"],
      child: some("/local/docs/b.md"),
    },
  ],
  truncated: false,
};

// The exhaustive fold a spec needs when exactly one level
// kind is legitimate — every other kind is a failed
// assertion naming what arrived instead.
const expectListLevel = (
  level: Level,
  body: (
    content: Readonly<{
      title: SoftStr;
      back: Option<SoftStr>;
      rows: ReadonlyArray<RowLink>;
      error: Option<SoftStr>;
    }>,
  ) => Assertion,
): Assertion =>
  match(level)(
    [
      listLevel$(),
      ({ content }): Assertion =>
        body({
          title: content.title,
          back: content.back,
          rows: content.rows,
          error: content.error,
        }),
    ],
    [
      menuLevel$(),
      (): Assertion => wrongKind("MenuLevel"),
    ],
    [
      boardLevel$(),
      (): Assertion => wrongKind("BoardLevel"),
    ],
    [
      detailLevel$(),
      (): Assertion => wrongKind("DetailLevel"),
    ],
  );

const wrongKind = (got: string): Assertion =>
  fail({
    matcher: "level kind",
    expected: "the lowered kind",
    actual: got,
    message: `the lowering produced a ${got}`,
  });

test("the describe lowering feeds the Scene: only contained children become rows", () =>
  expectListLevel(
    qfsLevel(
      view,
      some("/"),
      (child) => `/resolve/qfs:${child}`,
    ),
    (content) =>
      all([
        toBe(2)(content.rows.length),
        toEqual([
          "/resolve/qfs:/local/docs/a.md",
          "/resolve/qfs:/local/docs/b.md",
        ])(content.rows.map((r) => r.href)),
        // the first cell labels the row; an empty
        // first cell falls back to the child path
        toEqual([
          "/local/docs/a.md",
          "/local/docs/b.md",
        ])(content.rows.map((r) => r.row.label)),
        toBe("/local/docs")(content.title),
        toEqual(some("/"))(content.back),
      ]),
  ));

test("a qfs column that failed still gets a level carrying qfs's own words", () =>
  expectListLevel(
    qfsErrorLevel(
      "/nosuch",
      some("/"),
      "qfs: no driver is mounted for /nosuch (unknown_mount)",
    ),
    (content) =>
      all([
        toBe("/nosuch")(content.title),
        toEqual(
          some(
            "qfs: no driver is mounted for /nosuch (unknown_mount)",
          ),
        )(content.error),
        toBe(0)(content.rows.length),
      ]),
  ));

test("a declared resource lowers to a ListLevel that carries its error truth", () =>
  expectListLevel(
    resourceLevel(
      "Tickets",
      none(),
      some("parse error"),
    ),
    (content) =>
      all([
        toBe("Tickets")(content.title),
        toEqual(some("parse error"))(
          content.error,
        ),
        toBe(0)(content.rows.length),
      ]),
  ));

test("a document lowers to a DetailLevel whose identity is its path", () => {
  const level = docLevel(
    "docs/adr/index.md",
    some("/"),
  );
  return match(level)(
    [
      detailLevel$(),
      ({ content }): Assertion =>
        all([
          toBe("docs/adr/index.md")(
            content.title,
          ),
          toEqual(some("/"))(content.back),
        ]),
    ],
    [
      menuLevel$(),
      (): Assertion => wrongKind("MenuLevel"),
    ],
    [
      listLevel$(),
      (): Assertion => wrongKind("ListLevel"),
    ],
    [
      boardLevel$(),
      (): Assertion => wrongKind("BoardLevel"),
    ],
  );
});

// The pipeline proof: OUR lowering feeds the ENGINE's own crumb
// projection, and the engine reads the prefix-closed trail back out of
// it — crumb i links to the address at which level i is the deepest
// (the NEXT level's back), and the deepest crumb is where you are.
test("crumbsOf reads the prefix-closed trail out of the lowered scene", () => {
  const scene = sceneOf("qfs-viewer", [
    rootLevel("qfs-viewer", []),
    docLevel("docs/a.md", some("/")),
    docLevel(
      "docs/b.md",
      some("/resolve/docs/a.md"),
    ),
    qfsLevel(
      view,
      some("/resolve/docs/a.md,docs/b.md"),
      (child) => `/resolve/qfs:${child}`,
    ),
  ]);
  const crumbs = crumbsOf(scene);
  return all([
    toEqual([
      "qfs-viewer",
      "docs/a.md",
      "docs/b.md",
      "/local/docs",
    ])(crumbs.map((c) => c.label)),
    toEqual([
      some("/"),
      some("/resolve/docs/a.md"),
      some("/resolve/docs/a.md,docs/b.md"),
      none(),
    ])(crumbs.map((c) => c.to)),
  ]);
});
