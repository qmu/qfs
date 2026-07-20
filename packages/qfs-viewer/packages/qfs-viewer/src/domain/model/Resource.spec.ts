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
  asResourceConfig,
  asResourceTable,
} from "#qfs-viewer/domain/model/Resource";

// The real shape, copied from a real run:
//   $ qfs run '/local/… |> select name, size, is_dir'
const QFS_ANSWER = {
  schema: [
    { name: "name", type: "text" },
    { name: "size", type: "int" },
    { name: "is_dir", type: "bool" },
  ],
  rows: [
    {
      name: "CLAUDE.md",
      size: 2363,
      is_dir: false,
    },
    { name: "docs", size: 0, is_dir: true },
  ],
  meta: { row_count: 2, truncated: false },
};

test("qfs's answer reads into a table", () =>
  andThen(
    shouldBeOk()(asResourceTable(QFS_ANSWER)),
    (t) =>
      all([
        check(
          t.columns,
          toEqual([
            { name: "name", type: "text" },
            { name: "size", type: "int" },
            { name: "is_dir", type: "bool" },
          ]),
        ),
        check(t.rows.length, toBe(2)),
        check(t.truncated, toBe(false)),
      ]),
  ));

// qfs types its columns and returns JSON that matches. Flattening rows to
// strings at the boundary would throw away the types qfs went to the trouble
// of reporting; only the renderer needs text.
test("a row keeps its values as the types qfs reported", () =>
  andThen(
    shouldBeOk()(asResourceTable(QFS_ANSWER)),
    (t) =>
      all([
        check(t.rows[0]?.["size"], toBe(2363)),
        check(t.rows[0]?.["is_dir"], toBe(false)),
        check(t.rows[1]?.["is_dir"], toBe(true)),
      ]),
  ));

// A truncated result the reader is not told about is a lie by omission: they
// would read a partial table as the whole answer.
test("truncation is carried, because a reader must be told", () =>
  andThen(
    shouldBeOk()(
      asResourceTable({
        ...QFS_ANSWER,
        meta: { truncated: true },
      }),
    ),
    (t) => check(t.truncated, toBe(true)),
  ));

// "the query you declared has a syntax error" is exactly what the person who
// wrote it needs to read. Flattening it to "could not load" hides the answer.
test("qfs's own error is reported as itself, with its code", () =>
  andThen(
    shouldBeErr()(
      asResourceTable({
        error: {
          code: "parse_error",
          kind: "parse",
          message:
            "the grammar did not expect this token here",
        },
      }),
    ),
    (e) =>
      all([
        toBe(true)(
          e.content.message.includes(
            "the grammar did not expect this token here",
          ),
        ),
        toBe(true)(
          e.content.message.includes(
            "parse_error",
          ),
        ),
      ]),
  ));

// qfs is another program; its output is a boundary, and a shape we do not
// recognise is a typed failure rather than a crash three frames up.
test("a shape qfs never promised is a typed failure, not a throw", () =>
  all([
    check(asResourceTable("nope"), shouldBeErr()),
    check(asResourceTable(null), shouldBeErr()),
    check(asResourceTable({}), shouldBeErr()),
    // a statement that answers with no rows at all
    check(
      asResourceTable({ schema: [] }),
      shouldBeErr(),
    ),
    check(
      asResourceTable({
        schema: [{ type: "text" }],
        rows: [],
      }),
      shouldBeErr(),
    ),
  ]));

test("a column with no type reads as unknown rather than failing", () =>
  andThen(
    shouldBeOk()(
      asResourceTable({
        schema: [{ name: "x" }],
        rows: [],
      }),
    ),
    (t) =>
      check(t.columns[0]?.type, toBe("unknown")),
  ));

test("a declared resource is validated", () =>
  andThen(
    shouldBeOk()(
      asResourceConfig(
        {
          name: "repo-files",
          label: "Repository files",
          query: "/local/tmp |> select name",
        },
        0,
      ),
    ),
    (r) =>
      all([
        check(r.name, toBe("repo-files")),
        check(r.label, toBe("Repository files")),
      ]),
  ));

test("a resource with no label labels itself with its name", () =>
  andThen(
    shouldBeOk()(
      asResourceConfig(
        {
          name: "users",
          query: "/x |> select a",
        },
        0,
      ),
    ),
    (r) => check(r.label, toBe("users")),
  ));

// The name is addressed in a URL and must be distinguishable from a document
// path at a glance, which is why it is a slug.
test("a name that could not be a URL segment is refused", () =>
  all([
    check(
      asResourceConfig(
        {
          name: "not a slug",
          query: "/x |> select a",
        },
        0,
      ),
      shouldBeErr(),
    ),
    check(
      asResourceConfig(
        {
          name: "docs/a.md",
          query: "/x |> select a",
        },
        0,
      ),
      shouldBeErr(),
    ),
    check(
      asResourceConfig(
        { name: "", query: "/x |> select a" },
        0,
      ),
      shouldBeErr(),
    ),
  ]));

test("a resource with no statement is refused, naming its index", () =>
  andThen(
    shouldBeErr()(
      asResourceConfig({ name: "users" }, 2),
    ),
    (e) =>
      toBe(true)(
        e.content.message.includes(
          "resources[2].query",
        ),
      ),
  ));

// The query is NOT parsed here on purpose: qfs owns that grammar and reports a
// structured parse_error for a bad statement. A second opinion in this
// repository would be a worse copy that drifts.
test("a syntactically wrong statement is accepted here and refused by qfs", () =>
  check(
    asResourceConfig(
      {
        name: "x",
        query: "this is not qfs at all",
      },
      0,
    ),
    shouldBeOk(),
  ));
