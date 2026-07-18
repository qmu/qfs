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
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import { listCollection } from "#qfs-viewer/domain/usecase/listCollection";
import {
  parseListQuery,
  defaultLimit,
  maxLimit,
} from "#qfs-viewer/domain/model/Query";
import { documentPathString } from "#qfs-viewer/domain/model/Vocabulary";

// Each test builds its own tree — no shared dataset (workaholic:implementation
// / test).
const corpus = () =>
  scan(
    fakeFileSystem({
      "docs/a.md":
        "---\ntype: bugfix\nlayer:\n  - Domain\n  - Infrastructure\n---\nalpha",
      "docs/b.md":
        "---\ntype: enhancement\nlayer:\n  - Domain\n---\nbravo",
      "docs/c.md":
        "---\ntype: bugfix\n---\ncharlie",
      "docs/d.md": "# no front matter\ndelta",
    }),
  );

const pathsOf = (
  r: ReturnType<typeof listCollection>,
) =>
  r.contents.map((d) =>
    documentPathString(d.content.path),
  );

const query = (
  params: Record<string, string>,
) => {
  const q = parseListQuery(params);
  return q.__tag === "Ok" ? q.content : undefined;
};

test("parseListQuery defaults an empty bag", () =>
  andThen(shouldBeOk()(parseListQuery({})), (q) =>
    all([
      check(q.limit, toBe(defaultLimit)),
      check(q.offset, toBe(0)),
      check(q.orderDir, toBe("asc")),
      check(q.q.__tag, toBe("None")),
      check(q.tags, toEqual([])),
    ]),
  ));

test("parseListQuery rejects a non-numeric limit rather than clamping it", () =>
  check(
    parseListQuery({ limit: "abc" }),
    shouldBeErr(),
  ));

// `Number("")` is 0 and `Number(" 1 ")` is 1 — both would sail through a bare
// `Number(raw)` as a plausible value.
test("parseListQuery rejects a limit that only looks numeric", () =>
  all([
    check(
      parseListQuery({ limit: "" }),
      shouldBeErr(),
    ),
    check(
      parseListQuery({ limit: " 1 " }),
      shouldBeErr(),
    ),
  ]));

test("parseListQuery rejects a limit past the ceiling", () =>
  all([
    check(
      parseListQuery({
        limit: String(maxLimit + 1),
      }),
      shouldBeErr(),
    ),
    check(
      parseListQuery({ limit: "0" }),
      shouldBeErr(),
    ),
  ]));

test("parseListQuery rejects an unknown order", () =>
  check(
    parseListQuery({ order: "sideways" }),
    shouldBeErr(),
  ));

test("parseListQuery reads every unreserved parameter as a front-matter filter", () =>
  andThen(
    shouldBeOk()(
      parseListQuery({
        limit: "5",
        type: "bugfix",
        layer: "Domain",
      }),
    ),
    (q) =>
      check(
        q.tags,
        toEqual([
          { key: "type", value: "bugfix" },
          { key: "layer", value: "Domain" },
        ]),
      ),
  ));

test("an empty q filters on nothing rather than on the empty string", () =>
  andThen(
    shouldBeOk()(parseListQuery({ q: "" })),
    (q) => check(q.q.__tag, toBe("None")),
  ));

test("listCollection with no filter returns the whole corpus, path-ordered", () => {
  const q = query({});
  return q === undefined
    ? check(false, toBe(true))
    : all([
        check(
          pathsOf(listCollection(corpus(), q)),
          toEqual([
            "docs/a.md",
            "docs/b.md",
            "docs/c.md",
            "docs/d.md",
          ]),
        ),
        check(
          listCollection(corpus(), q).totalCount,
          toBe(4),
        ),
      ]);
});

test("a tag filter selects on a scalar front-matter value", () => {
  const q = query({ type: "bugfix" });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(listCollection(corpus(), q)),
        toEqual(["docs/a.md", "docs/c.md"]),
      );
});

// The mission's tag-group read: a sequence answers to any of its members.
test("a tag filter matches any member of a front-matter sequence", () => {
  const q = query({ layer: "Infrastructure" });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(listCollection(corpus(), q)),
        toEqual(["docs/a.md"]),
      );
});

test("tag filters are ANDed", () => {
  const q = query({
    type: "bugfix",
    layer: "Domain",
  });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(listCollection(corpus(), q)),
        toEqual(["docs/a.md"]),
      );
});

// Load-bearing while plgg-md's subset declines the workaholic format: those
// documents carry `None` front matter, so they are listed but never faceted.
test("a document with no front matter matches no tag filter", () => {
  const q = query({ type: "bugfix" });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(
          listCollection(corpus(), q),
        ).includes("docs/d.md"),
        toBe(false),
      );
});

test("free text matches the source, case-insensitively", () => {
  const q = query({ q: "BRAVO" });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(listCollection(corpus(), q)),
        toEqual(["docs/b.md"]),
      );
});

test("text and tag filters combine", () => {
  const q = query({ q: "alpha", type: "bugfix" });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(listCollection(corpus(), q)),
        toEqual(["docs/a.md"]),
      );
});

test("limit and offset page the result, and totalCount ignores the window", () => {
  const q = query({ limit: "2", offset: "1" });
  return q === undefined
    ? check(false, toBe(true))
    : all([
        check(
          pathsOf(listCollection(corpus(), q)),
          toEqual(["docs/b.md", "docs/c.md"]),
        ),
        check(
          listCollection(corpus(), q).totalCount,
          toBe(4),
        ),
      ]);
});

test("an offset past the end is an empty page, not an error", () => {
  const q = query({ offset: "99" });
  return q === undefined
    ? check(false, toBe(true))
    : all([
        check(
          pathsOf(listCollection(corpus(), q)),
          toEqual([]),
        ),
        check(
          listCollection(corpus(), q).totalCount,
          toBe(4),
        ),
      ]);
});

test("order=desc reverses the path order", () => {
  const q = query({ order: "desc", limit: "2" });
  return q === undefined
    ? check(false, toBe(true))
    : check(
        pathsOf(listCollection(corpus(), q)),
        toEqual(["docs/d.md", "docs/c.md"]),
      );
});

test("totalCount counts under the filter, not the corpus", () => {
  const q = query({ type: "bugfix", limit: "1" });
  return q === undefined
    ? check(false, toBe(true))
    : all([
        check(
          listCollection(corpus(), q).totalCount,
          toBe(2),
        ),
        check(
          listCollection(corpus(), q).contents,
          (c) => toBe(1)(c.length),
        ),
      ]);
});
