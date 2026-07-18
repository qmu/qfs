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
import { isOk, some, none } from "plgg";
import {
  isQfsPath,
  asQfsPath,
  asResourceDescribe,
  containedChild,
  lowerToDefaultView,
} from "#qfs-viewer/domain/model/Describe";
import { asResourceTable } from "#qfs-viewer/domain/model/Resource";

// The real shape, copied from a real run:
//   $ qfs describe /local/home/user/docs --json
const QFS_DESCRIBE = {
  path: "/local/home/user/docs",
  archetype: "blob_namespace",
  native_verbs: "UPSERT REMOVE LS CP MV RM",
  columns: [
    { name: "name", ty: "Text", nullable: false },
    { name: "path", ty: "Text", nullable: false },
    { name: "size", ty: "Int", nullable: false },
  ],
  verbs: { select: false, upsert: true },
  procedures: [],
  pushdown: { project: true },
};

test("qfs's describe answer reads into the slice the viewer needs", () =>
  andThen(
    shouldBeOk()(
      asResourceDescribe(QFS_DESCRIBE),
    ),
    (d) =>
      all([
        check(
          d.path,
          toBe("/local/home/user/docs"),
        ),
        check(
          d.archetype,
          toBe("blob_namespace"),
        ),
        check(
          d.columns,
          toEqual([
            { name: "name", type: "Text" },
            { name: "path", type: "Text" },
            { name: "size", type: "Int" },
          ]),
        ),
      ]),
  ));

// qfs's own words, verbatim: "no driver is mounted for /nosuch" is exactly
// what the person who typed the path needs to read.
test("a describe error is reported as qfs said it", () =>
  andThen(
    shouldBeErr()(
      asResourceDescribe({
        error: {
          code: "unknown_mount",
          kind: "capability",
          message:
            "no driver is mounted for `/nosuch` (describe registry)",
          path: "/nosuch",
        },
      }),
    ),
    (e) =>
      all([
        check(
          e.content.message.includes(
            "no driver is mounted",
          ),
          toBe(true),
        ),
        check(
          e.content.message.includes(
            "unknown_mount",
          ),
          toBe(true),
        ),
      ]),
  ));

test("a describe with no path is a typed failure, not a crash", () =>
  all([
    check(
      asResourceDescribe(undefined),
      shouldBeErr(),
    ),
    check(
      asResourceDescribe({ archetype: "x" }),
      shouldBeErr(),
    ),
    check(
      asResourceDescribe({
        path: "/x",
        columns: [{ ty: "Text" }],
      }),
      shouldBeErr(),
    ),
  ]));

// The charset is what makes embedding the path in `<path> |> limit N`
// injection-proof: no whitespace, quotes, or pipes can ride in.
test("a qfs path is absolute and spelled from the closed charset", () =>
  all([
    check(
      isQfsPath("/local/home/user/docs"),
      toBe(true),
    ),
    check(
      isQfsPath("/git/repo@v1.2/src"),
      toBe(true),
    ),
    check(
      isQfsPath("/sql/pg/orders"),
      toBe(true),
    ),
    check(
      isQfsPath("relative/path"),
      toBe(false),
    ),
    check(isQfsPath("/"), toBe(false)),
    check(isQfsPath(""), toBe(false)),
    check(
      isQfsPath("/a |> remove /b"),
      toBe(false),
    ),
    check(isQfsPath("/a'b"), toBe(false)),
    check(isQfsPath("/a b"), toBe(false)),
    check(isQfsPath("/a//b"), toBe(false)),
    check(isQfsPath("/a/b/"), toBe(false)),
  ]));

// One resource, one address: dot-segments would mint aliases for the same
// node, and the canonical address is the trail's anchor.
test("dot segments are refused, not normalized", () =>
  all([
    check(isQfsPath("/a/../b"), toBe(false)),
    check(isQfsPath("/a/./b"), toBe(false)),
    // dots INSIDE a name are ordinary spelling
    check(
      isQfsPath("/local/a/readme.md"),
      toBe(true),
    ),
  ]));

test("asQfsPath names the rule in its refusal", () =>
  andThen(
    shouldBeErr()(asQfsPath("not absolute")),
    (e) =>
      check(
        e.content.message.includes(
          "not a qfs path",
        ),
        toBe(true),
      ),
  ));

// ---- the lowering: (describe, rows) -> default view ----

const QFS_ANSWER = {
  schema: [
    { name: "name", type: "text" },
    { name: "path", type: "text" },
    { name: "size", type: "int" },
  ],
  rows: [
    {
      name: "index.md",
      path: "/local/home/user/docs/index.md",
      size: 12092,
    },
    {
      name: "assets",
      path: "/local/home/user/docs/assets",
      size: 0,
    },
    // a row that names something OUTSIDE the viewed path must not link —
    // containment is the whole rule
    {
      name: "escape",
      path: "/local/home/user/other",
      size: 1,
    },
  ],
  meta: { row_count: 3, truncated: false },
};

const view = () => {
  const d = asResourceDescribe(QFS_DESCRIBE);
  const t = asResourceTable(QFS_ANSWER);
  if (!isOk(d) || !isOk(t)) {
    throw new Error("fixture did not parse");
  }
  return lowerToDefaultView(d.content, t.content);
};

// Index access under noUncheckedIndexedAccess: the fixture rows exist, and a
// spec may say so out loud.
const rowAt = (
  v: ReturnType<typeof view>,
  i: number,
) => {
  const row = v.rows[i];
  if (row === undefined) {
    throw new Error(`fixture has no row ${i}`);
  }
  return row;
};

test("the lowering is thin: the run's schema orders the cells", () => {
  const v = view();
  return all([
    check(v.path, toBe("/local/home/user/docs")),
    check(v.archetype, toBe("blob_namespace")),
    check(
      v.columns.map((c) => c.name),
      toEqual(["name", "path", "size"]),
    ),
    check(v.rows.length, toBe(3)),
    check(
      rowAt(v, 0).cells,
      toEqual([
        "index.md",
        "/local/home/user/docs/index.md",
        "12092",
      ]),
    ),
    check(v.truncated, toBe(false)),
  ]);
});

// The containment rule: a row whose `path` extends the viewed path is a
// click; anything else — outside paths, missing columns — is not.
test("a contained row links and an outside row does not", () => {
  const v = view();
  return all([
    check(
      rowAt(v, 0).child,
      toEqual(
        some("/local/home/user/docs/index.md"),
      ),
    ),
    check(
      rowAt(v, 1).child,
      toEqual(
        some("/local/home/user/docs/assets"),
      ),
    ),
    check(rowAt(v, 2).child, toEqual(none())),
  ]);
});

test("a row with no path column has no link — tables are browsable, not clickable, until /resolve", () =>
  check(
    containedChild("/sql/pg/orders", {
      id: 7,
      total: 120,
    }),
    toEqual(none()),
  ));

test("a forged path column cannot smuggle a bad segment into the trail", () =>
  all([
    check(
      containedChild("/local/a", {
        path: "/local/a/ok but spaced",
      }),
      toEqual(none()),
    ),
    check(
      containedChild("/local/a", {
        path: "/local/a/../escape",
      }),
      toEqual(none()),
    ),
    check(
      containedChild("/local/a", { path: 42 }),
      toEqual(none()),
    ),
  ]));

// Display truncation, not data truncation: a blob's content column is the
// whole file base64-ed and must not eat the column.
test("a huge cell is truncated for display with a visible marker", () => {
  const d = asResourceDescribe(QFS_DESCRIBE);
  const t = asResourceTable({
    schema: [{ name: "content", type: "bytes" }],
    rows: [{ content: "x".repeat(500) }],
    meta: { truncated: false },
  });
  if (!isOk(d) || !isOk(t)) {
    throw new Error("fixture did not parse");
  }
  const v = lowerToDefaultView(
    d.content,
    t.content,
  );
  const cell = rowAt(v, 0).cells[0] ?? "";
  return all([
    check(cell.length, toBe(161)),
    check(cell.endsWith("…"), toBe(true)),
  ]);
});

// Leniency where qfs may say less: archetype and columns are display facts,
// and their absence must not make a path unbrowsable.
test("a describe without archetype or columns still reads", () =>
  andThen(
    shouldBeOk()(
      asResourceDescribe({ path: "/x/y" }),
    ),
    (d) =>
      all([
        check(d.archetype, toBe("unknown")),
        check(d.columns, toEqual([])),
        check(d.path, toBe("/x/y")),
      ]),
  ));

test("a column whose ty is missing reads as unknown", () =>
  andThen(
    shouldBeOk()(
      asResourceDescribe({
        path: "/x",
        columns: [{ name: "c" }],
      }),
    ),
    (d) =>
      check(
        d.columns,
        toEqual([{ name: "c", type: "unknown" }]),
      ),
  ));

// The cell conversion is total over what JSON can hold.
test("boolean and structured cells render as readable text", () => {
  const d = asResourceDescribe({ path: "/x" });
  const t = asResourceTable({
    schema: [
      { name: "flag", type: "bool" },
      { name: "blob", type: "json" },
      { name: "gone", type: "text" },
    ],
    rows: [
      {
        flag: true,
        blob: { deep: [1, 2] },
        gone: null,
      },
    ],
    meta: { truncated: false },
  });
  if (!isOk(d) || !isOk(t)) {
    throw new Error("fixture did not parse");
  }
  const v = lowerToDefaultView(
    d.content,
    t.content,
  );
  return check(
    rowAt(v, 0).cells,
    toEqual(["true", '{"deep":[1,2]}', ""]),
  );
});
