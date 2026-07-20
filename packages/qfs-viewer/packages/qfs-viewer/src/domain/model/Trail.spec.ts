import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import { some, none, isOk } from "plgg";
import {
  type Stop,
  type Trail,
  parseTrail,
  parseResolvePath,
  formatTrail,
  trailUrl,
  openFrom,
  docStop,
  qfsStop,
} from "#qfs-viewer/domain/model/Trail";
import {
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

// The brand exists precisely so a trail cannot hold a raw string; the tests
// have to go through the boundary like everyone else.
// A trail holds `Stop`s now, not raw paths — a document and a qfs resource are
// different things and the union says so. The fixture builds the Doc stop,
// which is what every test here was already about.
const p = (raw: string): Stop => {
  const r = asDocumentPath(raw);
  if (!isOk(r)) {
    throw new Error(
      `fixture is not a path: ${raw}`,
    );
  }
  return docStop(r.content);
};

// Renders a trail for comparison: a document by its path, a resource by its
// `res:` form, a qfs stop by its `qfs:` form, so a test can assert on any.
const strs = (t: Trail): ReadonlyArray<string> =>
  t.map((stop) =>
    stop.__tag === "Doc"
      ? documentPathString(stop.path)
      : stop.__tag === "Resource"
        ? `res:${stop.name}`
        : `qfs:${stop.path}`,
  );

test("an absent cols parameter is an empty trail, not an error", () =>
  check(parseTrail(none()), toEqual([])));

test("an empty cols parameter is an empty trail", () =>
  check(parseTrail(some("")), toEqual([])));

test("a trail round-trips through the URL", () => {
  const trail = [
    p("docs/a.md"),
    p("docs/deep/b.md"),
  ];
  return check(
    strs(parseTrail(some(formatTrail(trail)))),
    toEqual(["docs/a.md", "docs/deep/b.md"]),
  );
});

// The mission asks for a traversal legible in the URL, so slashes stay
// slashes: `/resolve/docs/a.md,docs/b.md` is readable, `docs%2Fa.md` is not.
test("slashes stay readable in the URL", () =>
  all([
    check(
      formatTrail([p("docs/a.md")]),
      toBe("docs/a.md"),
    ),
    check(
      trailUrl([p("docs/a.md"), p("docs/b.md")]),
      toBe("/resolve/docs/a.md,docs/b.md"),
    ),
  ]));

// The separator cannot be forged. A comma in a filename is legal, and if it
// survived into the URL it would split one document into two phantom columns.
test("a comma in a filename cannot forge the separator", () => {
  const trail = [
    p("docs/a,b.md"),
    p("docs/c.md"),
  ];
  const url = formatTrail(trail);
  return all([
    // the real comma is escaped, the separator is not
    check(url, toBe("docs/a%2Cb.md,docs/c.md")),
    // and it comes back as ONE document, not two
    check(
      strs(parseTrail(some(url))),
      toEqual(["docs/a,b.md", "docs/c.md"]),
    ),
  ]);
});

test("a percent in a filename round-trips too", () => {
  const trail = [p("docs/50%.md")];
  return check(
    strs(parseTrail(some(formatTrail(trail)))),
    toEqual(["docs/50%.md"]),
  );
});

test("the empty trail is the bare root, not an empty query", () =>
  check(trailUrl([]), toBe("/")));

// Untrusted input: a hand-edited URL or an aged bookmark. Losing one column
// beats answering 400 to someone whose link went stale.
test("a segment that is not a document path drops out and the rest still opens", () =>
  all([
    check(
      strs(
        parseTrail(
          some(
            "docs/a.md,/absolute.md,docs/b.md",
          ),
        ),
      ),
      toEqual(["docs/a.md", "docs/b.md"]),
    ),
    check(
      strs(
        parseTrail(
          some("docs/a.md,../escape.md"),
        ),
      ),
      toEqual(["docs/a.md"]),
    ),
    check(
      strs(
        parseTrail(some("docs/a.md,notes.txt")),
      ),
      toEqual(["docs/a.md"]),
    ),
  ]));

// The gate: "opens it in a new column to the right without discarding the
// previous one".
test("opening from the corpus list starts a one-document trail", () =>
  check(
    strs(openFrom([], -1, p("docs/a.md"))),
    toEqual(["docs/a.md"]),
  ));

test("opening from the last column appends without discarding it", () =>
  check(
    strs(
      openFrom(
        [p("docs/a.md")],
        0,
        p("docs/b.md"),
      ),
    ),
    toEqual(["docs/a.md", "docs/b.md"]),
  ));

// A trail, not a pile: following a link from an earlier column means the
// columns to its right are no longer how you got here.
test("opening from an earlier column drops what was to its right", () =>
  check(
    strs(
      openFrom(
        [
          p("docs/a.md"),
          p("docs/b.md"),
          p("docs/c.md"),
        ],
        0,
        p("docs/z.md"),
      ),
    ),
    toEqual(["docs/a.md", "docs/z.md"]),
  ));

test("opening from the corpus list with columns open resets the trail", () =>
  check(
    strs(
      openFrom(
        [p("docs/a.md"), p("docs/b.md")],
        -1,
        p("docs/z.md"),
      ),
    ),
    toEqual(["docs/z.md"]),
  ));

// The input trail is untouched — every navigation is a new value, like the
// index it reads.
test("openFrom returns a new trail and leaves the old one intact", () => {
  const before = [p("docs/a.md")];
  const after = openFrom(
    before,
    0,
    p("docs/b.md"),
  );
  return all([
    check(strs(before), toEqual(["docs/a.md"])),
    check(
      strs(after),
      toEqual(["docs/a.md", "docs/b.md"]),
    ),
  ]);
});

// ---- qfs stops: any describable path joins the trail ----

// The mission's generic browsing: a qfs path is a third kind of stop, and
// its address rides the same `cols` value the other two ride — legible, so
// `qfs:/local/home/user/docs` reads as what it is.
test("a qfs stop round-trips through the URL beside the other kinds", () => {
  const trail = [
    p("docs/a.md"),
    qfsStop("/local/home/user/docs"),
  ];
  const url = formatTrail(trail);
  return all([
    check(
      url,
      toBe("docs/a.md,qfs:/local/home/user/docs"),
    ),
    check(
      strs(parseTrail(some(url))),
      toEqual([
        "docs/a.md",
        "qfs:/local/home/user/docs",
      ]),
    ),
  ]);
});

// The same gate as the /qfs route: a hand-edited segment that is not a qfs
// path drops out, and nothing that failed the gate can reach a statement.
test("a forged qfs segment drops out and the rest still opens", () =>
  all([
    check(
      strs(
        parseTrail(
          some(
            "docs/a.md,qfs:/a |> remove /b,docs/b.md",
          ),
        ),
      ),
      toEqual(["docs/a.md", "docs/b.md"]),
    ),
    check(
      strs(parseTrail(some("qfs:relative"))),
      toEqual([]),
    ),
    check(
      strs(parseTrail(some("qfs:/a/../b"))),
      toEqual([]),
    ),
  ]));

test("opening a qfs child from a qfs column appends like any other stop", () =>
  check(
    strs(
      openFrom(
        [qfsStop("/local/a")],
        0,
        qfsStop("/local/a/b"),
      ),
    ),
    toEqual(["qfs:/local/a", "qfs:/local/a/b"]),
  ));

// ---- the /resolve address: prefix closure, click = segment append ----

// The acceptance item, as codec algebra: the address of any trail PREFIX is
// the corresponding PREFIX of the address, cut at a separator — so every
// prefix of a valid address is a valid address, and it names exactly the
// first columns of the longer one.
test("the address is prefix-closed: a trail prefix's address is the address's prefix", () => {
  const trail = [
    p("docs/a.md"),
    p("docs/deep/b.md"),
    qfsStop("/local/repo"),
  ];
  const full = trailUrl(trail);
  const two = trailUrl(trail.slice(0, 2));
  const one = trailUrl(trail.slice(0, 1));
  return all([
    check(
      full,
      toBe(
        "/resolve/docs/a.md,docs/deep/b.md,qfs:/local/repo",
      ),
    ),
    check(full.startsWith(`${two},`), toBe(true)),
    check(two.startsWith(`${one},`), toBe(true)),
    // and each prefix address reads back as exactly the prefix trail
    check(
      strs(parseResolvePath(two)),
      toEqual(["docs/a.md", "docs/deep/b.md"]),
    ),
    check(
      strs(parseResolvePath(one)),
      toEqual(["docs/a.md"]),
    ),
  ]);
});

// "a click is a segment appended to the address" — literally: opening from
// the last column changes the address by one appended segment and nothing
// else.
test("a click from the last column appends one segment to the address", () => {
  const trail = [p("docs/a.md")];
  const clicked = openFrom(
    trail,
    trail.length - 1,
    qfsStop("/local/repo"),
  );
  return check(
    trailUrl(clicked),
    toBe(`${trailUrl(trail)},qfs:/local/repo`),
  );
});

test("a pasted /resolve address reads back as the same trail", () => {
  const trail = [
    p("docs/a,b.md"),
    p("docs/50%.md"),
    qfsStop("/local/home/user/docs"),
  ];
  return check(
    strs(parseResolvePath(trailUrl(trail))),
    toEqual([
      "docs/a,b.md",
      "docs/50%.md",
      "qfs:/local/home/user/docs",
    ]),
  );
});

// Display state is PROVABLY absent from the address: the grammar has no
// parameter slot at all. The address a trail renders to carries no query
// component, and the codec reads the path alone — folding, sort order and
// highlights have nowhere to ride.
test("the address has no parameter slot for display state to hide in", () => {
  const url = trailUrl([
    p("docs/a.md"),
    qfsStop("/local/repo"),
  ]);
  return all([
    check(url.includes("?"), toBe(false)),
    check(url.includes("&"), toBe(false)),
    check(url.includes("="), toBe(false)),
  ]);
});

// The address is hand-editable input, and decodeURIComponent THROWS on a
// malformed escape — a bad segment must drop out, never 500.
test("a segment with a malformed percent-escape drops out instead of throwing", () =>
  all([
    check(
      strs(
        parseResolvePath(
          "/resolve/docs/a.md,docs/%zz.md",
        ),
      ),
      toEqual(["docs/a.md"]),
    ),
    check(
      strs(parseTrail(some("docs/%2"))),
      toEqual([]),
    ),
  ]));

// A path this codec does not own is the empty trail — the canonical spelling
// of "nothing open" stays `/`, and the route layer redirects to it.
test("a path outside /resolve is the empty trail", () =>
  all([
    check(
      parseResolvePath("/resolve"),
      toEqual([]),
    ),
    check(
      parseResolvePath("/resolve/"),
      toEqual([]),
    ),
    check(
      parseResolvePath("/other"),
      toEqual([]),
    ),
  ]));
