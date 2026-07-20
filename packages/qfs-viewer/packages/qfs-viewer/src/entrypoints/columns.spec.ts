// The mission's gate, pinned:
//
//   "The page lists markdown documents scanned from the working tree and
//    faceted by tag group; following a document link opens it in a new column
//    to the right without discarding the previous one, and the URL records the
//    traversal so a reload restores the same columns."
//
// Each clause below is one of those, asserted on rendered state.
import {
  test,
  all,
  toBe,
  shouldBeOk,
  andThen,
} from "plgg-test";
import {
  handle,
  getRequest,
  type ResponseBody,
} from "plgg-server";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import { indexRef } from "#qfs-viewer/domain/usecase/reload";
import { collectionRef } from "#qfs-viewer/domain/usecase/collection";
import { api } from "#qfs-viewer/entrypoints/api";
import { ok, err, invalidError } from "plgg";
import { asConfig } from "#qfs-viewer/domain/model/Config";
import { type ResourceRunner } from "#qfs-viewer/domain/model/Scan";

// A corpus with real front matter and real relative links between documents.
const CORPUS = {
  "docs/adr/index.md": [
    "---",
    "type: index",
    "---",
    "# ADRs",
    "",
    "- [0001](0001-npm-only.md)",
    "- [0002](0002-no-cache.md)",
    "",
  ].join("\n"),
  "docs/adr/0001-npm-only.md": [
    "---",
    "type: adr",
    "layer: [Config, Infrastructure]",
    "---",
    "# npm only",
    "",
    "See [the index](index.md) and [0002](0002-no-cache.md).",
    "",
  ].join("\n"),
  "docs/adr/0002-no-cache.md": [
    "---",
    "type: adr",
    "layer: [Infrastructure]",
    "---",
    "# no cache",
    "",
  ].join("\n"),
  "README.md": "# readme\n",
};

const app = () =>
  api(indexRef(scan(fakeFileSystem(CORPUS))));

const htmlOf = (r: {
  body: ResponseBody;
}): string =>
  typeof r.body === "string" ? r.body : "";

const statusOfResponse = (r: {
  status: { content: number };
}): number => r.status.content;

const getWithQuery = (
  path: string,
  query: Readonly<Record<string, string>>,
) => ({ ...getRequest(path), query });

// Every column of the engine strip carries exactly one sticky column header
// (the engine's `pm-colhead`); counting them counts the columns on screen,
// corpus column included.
const columnCount = (html: string): number =>
  (html.match(/class="pm-colhead"/g) ?? [])
    .length;

test("GET / lists the corpus and is a page, not a 404", async () =>
  andThen(
    shouldBeOk()(
      await handle(app(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            "docs/adr/0001-npm-only.md",
          ),
        ),
        // just the corpus column when nothing is open
        toBe(1)(columnCount(htmlOf(r))),
      ]),
  ));

// "faceted by tag group" — the dimensions are discovered from the corpus's own
// front matter, and each variation is a link.
test("GET / offers a facet per tag group, with counts", async () =>
  andThen(
    shouldBeOk()(
      await handle(app(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("<h2>type</h2>"),
        ),
        toBe(true)(
          htmlOf(r).includes("<h2>layer</h2>"),
        ),
        // a sequence puts one document on two variations of one dimension —
        // the thing a directory cannot do
        toBe(true)(
          htmlOf(r).includes(
            'href="/?layer=Infrastructure"',
          ),
        ),
        toBe(true)(htmlOf(r).includes("adr (2)")),
      ]),
  ));

test("a facet narrows the list to the documents carrying that variation", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery("/", { type: "adr" }),
      ),
    ),
    (r) =>
      all([
        // Both numbers: what is on screen, and the corpus it was drawn from.
        // `2 of 4` used to say this, and said it ambiguously — the left number
        // was the PAGE, so a 25-match filter on a 20 page also read `20 of 4x`
        // and never admitted the other 5 existed.
        toBe(true)(
          htmlOf(r).includes(
            "1–2 of 2 document(s) (4 in corpus)",
          ),
        ),
        toBe(false)(
          htmlOf(r).includes(">README.md</a>"),
        ),
      ]),
  ));

// A facet count is counted over the FILTERED set, so it is only true if the
// click keeps the filter. These two tests pin the count and the link to the
// same query: the pair was inconsistent once (`Config (1)` linked to
// `/?layer=Config`, which answered 4), and either one alone looks right.
test("a facet link ANDs with the filter already on", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery("/", { type: "adr" }),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            'href="/?type=adr&amp;layer=Infrastructure"',
          ),
        ),
        // the bare link would drop the type and page a different corpus
        toBe(false)(
          htmlOf(r).includes(
            'href="/?layer=Infrastructure"',
          ),
        ),
      ]),
  ));

test("an applied facet links to its own removal", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery("/", { type: "adr" }),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("× adr (2)"),
        ),
        // removing the only filter goes home; without this, drilling in is a
        // one-way trip into the empty set
        toBe(true)(
          htmlOf(r).includes('href="/"'),
        ),
      ]),
  ));

// The count says the documents are there; the pager is what makes them
// reachable. `limit=1` pages this 4-document corpus without needing 20 more.
test("the pager offers the documents past the first page", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery("/", { limit: "1" }),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            "1–1 of 4 document(s)",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            'href="/?limit=1&amp;offset=1"',
          ),
        ),
        // page 1 has nowhere back to
        toBe(false)(
          htmlOf(r).includes('class="prev"'),
        ),
      ]),
  ));

// THE TRAP the ticket named: a spec that pages with no column open and no
// facet applied passes while the pager silently drops both. Both bugs this
// column has shipped were interactions, and each spec tested one side alone.
test("a paging link carries the open columns AND the facet", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery(
          "/resolve/docs/adr/index.md",
          {
            type: "adr",
            limit: "1",
          },
        ),
      ),
    ),
    (r) =>
      all([
        // 1 of the 2 adr documents, and the corpus is 4
        toBe(true)(
          htmlOf(r).includes(
            "1–1 of 2 document(s) (4 in corpus)",
          ),
        ),
        // next carries the trail (as the path), the facet, and the page
        // size — dropping any one of them pages a corpus the reader is not
        // looking at
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/docs/adr/index.md?type=adr&amp;limit=1&amp;offset=1"',
          ),
        ),
      ]),
  ));

test("paging back to the first page drops the offset rather than sending offset=0", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery(
          "/resolve/docs/adr/index.md",
          {
            type: "adr",
            limit: "1",
            offset: "1",
          },
        ),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            "2–2 of 2 document(s) (4 in corpus)",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/docs/adr/index.md?type=adr&amp;limit=1"',
          ),
        ),
        // the last page offers no next
        toBe(false)(
          htmlOf(r).includes('class="next"'),
        ),
      ]),
  ));

// THE GATE: a link inside a document opens a NEW column to the right, and the
// column it was clicked from survives.
test("a link inside a document opens the next column without discarding this one", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest("/resolve/docs/adr/index.md"),
      ),
    ),
    (r) =>
      all([
        // the open document's link carries the CURRENT trail plus its target
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md"',
          ),
        ),
        toBe(2)(columnCount(htmlOf(r))),
      ]),
  ));

// The bug this pins: `docs/adr/index.md` writes `](0001-npm-only.md)`, meaning
// its NEIGHBOUR. Resolving that as root-relative pointed every ADR-index link
// at a nonexistent root-level document. Found by driving the real corpus.
test("a relative link resolves against the document that wrote it", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest("/resolve/docs/adr/index.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            "/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md",
          ),
        ),
        // NOT the root-level path the bug produced
        toBe(false)(
          htmlOf(r).includes(
            "/resolve/docs/adr/index.md,0001-npm-only.md",
          ),
        ),
      ]),
  ));

// The same document rendered at a different depth resolves its links
// differently, because depth is what changes.
test("the trail grows one column per link followed", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest(
          "/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(3)(columnCount(htmlOf(r))),
        // column 2's link appends ONE segment to this very address — the
        // acceptance item's "a click navigates to the same address plus one
        // segment", read off the href
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md,docs/adr/0002-no-cache.md"',
          ),
        ),
      ]),
  ));

// Prefix closure, rendered: the SAME address cut at its separator is a valid
// address, and it renders exactly the first columns of the longer one.
test("a prefix of the address renders a prefix of the columns", async () => {
  const full = await handle(
    app(),
    getRequest(
      "/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md",
    ),
  );
  const prefix = await handle(
    app(),
    getRequest("/resolve/docs/adr/index.md"),
  );
  return andThen(shouldBeOk()(full), (f) =>
    andThen(shouldBeOk()(prefix), (p) =>
      all([
        toBe(3)(columnCount(htmlOf(f))),
        toBe(2)(columnCount(htmlOf(p))),
        // the shared prefix renders the same column in both
        toBe(true)(
          htmlOf(f).includes('<h1 id="adrs">'),
        ),
        toBe(true)(
          htmlOf(p).includes('<h1 id="adrs">'),
        ),
      ]),
    ),
  );
});

// A trail, not a pile: following a link from an earlier column replaces what
// was to its right.
test("following a link from an earlier column drops the columns after it", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest(
          "/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md",
        ),
      ),
    ),
    (r) =>
      // column 1 (the index) still offers its own links at depth 0, so
      // following one yields a two-deep trail, not a four-deep one
      toBe(true)(
        htmlOf(r).includes(
          'href="/resolve/docs/adr/index.md,docs/adr/0002-no-cache.md"',
        ),
      ),
  ));

// "the URL records the traversal so a reload restores the same columns" — a
// reload is just the same GET, so this is literally the property.
test("the same URL restores the same columns", async () => {
  const a = await handle(
    app(),
    getRequest(
      "/resolve/docs/adr/index.md,docs/adr/0002-no-cache.md",
    ),
  );
  const b = await handle(
    app(),
    getRequest(
      "/resolve/docs/adr/index.md,docs/adr/0002-no-cache.md",
    ),
  );
  return andThen(shouldBeOk()(a), (first) =>
    andThen(shouldBeOk()(b), (second) =>
      all([
        toBe(true)(
          htmlOf(first) === htmlOf(second),
        ),
        toBe(3)(columnCount(htmlOf(first))),
      ]),
    ),
  );
});

// An external link must survive untouched — rewriting it into a column would
// be a bug that ate the web.
test("an external link is left exactly as the author wrote it", async () => {
  const withLink = api(
    indexRef(
      scan(
        fakeFileSystem({
          "docs/a.md":
            "# a\n\n[out](https://example.com/x.md)\n",
        }),
      ),
    ),
  );
  return andThen(
    shouldBeOk()(
      await handle(
        withLink,
        getRequest("/resolve/docs/a.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            'href="https://example.com/x.md"',
          ),
        ),
        toBe(false)(
          htmlOf(r).includes(
            "/resolve/docs/a.md,https:",
          ),
        ),
      ]),
  );
});

// The URL named it, so the screen owes the reader an answer about it.
test("a column for a document the corpus lost says so", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest("/resolve/docs/gone.md"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes("column-missing"),
        ),
        toBe(2)(columnCount(htmlOf(r))),
      ]),
  ));

// Untrusted input: a hand-edited URL, an aged bookmark. Losing one column
// beats a 400 to someone whose link went stale.
test("a garbage segment drops out and the rest of the trail still opens", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest(
          "/resolve/docs/adr/index.md,../escape.md,README.md",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        // corpus + the two valid documents
        toBe(3)(columnCount(htmlOf(r))),
      ]),
  ));

test("the columns page carries no-store like every other response", async () =>
  andThen(
    shouldBeOk()(
      await handle(app(), getRequest("/")),
    ),
    (r) =>
      toBe("no-store, must-revalidate")(
        r.headers["cache-control"],
      ),
  ));

test("headings inside a column are still numbered", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest(
          "/resolve/docs/adr/0001-npm-only.md",
        ),
      ),
    ),
    (r) =>
      toBe(true)(
        htmlOf(r).includes(
          '<h1 id="npm-only">1. npm only</h1>',
        ),
      ),
  ));

// The column-level twin of the 500 above: a document that will not render must
// not take the whole screen down. The other columns are fine, and this one says
// what is wrong with it.
test("a column whose document will not render says so and leaves the others alone", async () => {
  const withBad = api(
    indexRef(
      scan(
        fakeFileSystem({
          "docs/bad.md":
            "---\ntitle: x\nno closing fence",
          "docs/good.md": "# good\n",
        }),
      ),
    ),
  );
  return andThen(
    shouldBeOk()(
      await handle(
        withBad,
        getRequest(
          "/resolve/docs/bad.md,docs/good.md",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            "could not be rendered",
          ),
        ),
        // the good column still rendered
        toBe(true)(
          htmlOf(r).includes('<h1 id="good">'),
        ),
        toBe(3)(columnCount(htmlOf(r))),
      ]),
  );
});

// A facet link while columns are open must keep them open: narrowing the list
// is a filter on column 0, not a navigation away from everything.
test("a facet link carries the open columns with it", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest("/resolve/docs/adr/index.md"),
      ),
    ),
    (r) =>
      toBe(true)(
        htmlOf(r).includes(
          'href="/resolve/docs/adr/index.md?type=adr"',
        ),
      ),
  ));

// ---- qfs resources, alongside the markdown ----

// A fake runner returning the shape a real qfs returned, so these run without
// a qfs, a network, or a 7-second subprocess. The shape itself was copied from
// a real answer, so agreeing with the fake means agreeing with qfs.
const fakeRunner = (
  answer: unknown,
  described: unknown = {
    error: {
      code: "unknown_mount",
      message: "no driver is mounted",
    },
  },
): ResourceRunner => ({
  run: () => ok(answer),
  describe: () => ok(described),
});

const REAL_QFS_ANSWER = {
  schema: [
    { name: "name", type: "text" },
    { name: "size", type: "int" },
  ],
  rows: [
    { name: "CLAUDE.md", size: 2363 },
    { name: "README.md", size: 5133 },
  ],
  meta: { truncated: false },
};

const withResource = (
  answer: unknown = REAL_QFS_ANSWER,
) => {
  const c = asConfig({
    resources: [
      {
        name: "repo-files",
        label: "Repository files",
        query: "/local/tmp |> select name, size",
      },
    ],
  });
  if (c.__tag === "Err") {
    throw new Error(c.content.content.message);
  }
  return api(
    indexRef(scan(fakeFileSystem(CORPUS))),
    undefined,
    c.content,
    fakeRunner(answer),
  );
};

// "Alongside" — the mission's word. Two lists, because they are two kinds of
// thing: markdown this server indexed, and a live table qfs answers on ask.
test("a declared resource is listed beside the documents, not merged into them", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withResource(),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            "<h2>Resources</h2>",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "<h2>Documents</h2>",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/res:repo-files"',
          ),
        ),
        // and the documents are still there
        toBe(true)(
          htmlOf(r).includes("docs/adr/index.md"),
        ),
      ]),
  ));

test("opening a resource renders qfs's rows as a real table", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withResource(),
        getRequest("/resolve/res:repo-files"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        // the schema qfs reported, types and all
        toBe(true)(
          htmlOf(r).includes(
            "<th>name (text)</th>",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "<th>size (int)</th>",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "<td>CLAUDE.md</td>",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes("<td>2363</td>"),
        ),
      ]),
  ));

// A document and a resource in one trail is the whole claim: alongside, in the
// same URL, in the same screen.
test("a resource and a document open side by side in one trail", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withResource(),
        getRequest(
          "/resolve/docs/adr/index.md,res:repo-files",
        ),
      ),
    ),
    (r) =>
      all([
        // corpus + document + resource
        toBe(3)(columnCount(htmlOf(r))),
        toBe(true)(
          htmlOf(r).includes('<h1 id="adrs">'),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "<td>CLAUDE.md</td>",
          ),
        ),
      ]),
  ));

// qfs's own words, not "could not load": a parse error in the declared
// statement is exactly what the person who wrote it needs to read.
test("a qfs error is shown as qfs said it", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withResource({
          error: {
            code: "parse_error",
            message:
              "the grammar did not expect this token here",
          },
        }),
        getRequest("/resolve/res:repo-files"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            "the grammar did not expect this token here",
          ),
        ),
        // and the statement, so the author can see what ran
        toBe(true)(
          htmlOf(r).includes(
            "/local/tmp |&gt; select name, size",
          ),
        ),
      ]),
  ));

// A partial table read as a whole answer is a lie by omission.
test("a truncated result says so", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withResource({
          ...REAL_QFS_ANSWER,
          meta: { truncated: true },
        }),
        getRequest("/resolve/res:repo-files"),
      ),
    ),
    (r) =>
      toBe(true)(
        htmlOf(r).includes(
          "you are not seeing every row",
        ),
      ),
  ));

// qfs reaches mail, databases and cloud accounts. A resource appears because
// someone declared it; an undeclared name is not a thing this server will run.
test("a resource this repository never declared is refused, not run", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withResource(),
        getRequest("/resolve/res:whatever"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("column-missing"),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "declares no resource by that name",
          ),
        ),
      ]),
  ));

// The capability is the argument: an app built with no runner cannot reach
// qfs, and says so rather than half-working.
test("a server built without a runner cannot read resources at all", async () => {
  const c = asConfig({
    resources: [
      {
        name: "repo-files",
        query: "/local/tmp |> select name",
      },
    ],
  });
  if (c.__tag === "Err") {
    throw new Error(c.content.content.message);
  }
  const noRunner = api(
    indexRef(scan(fakeFileSystem(CORPUS))),
    undefined,
    c.content,
  );
  return andThen(
    shouldBeOk()(
      await handle(
        noRunner,
        getRequest("/resolve/res:repo-files"),
      ),
    ),
    (r) =>
      toBe(true)(
        htmlOf(r).includes(
          "built without a qfs runner",
        ),
      ),
  );
});

// No declaration, no section — a corpus that surfaces nothing is not told it
// has an empty Resources list.
test("a repository that declares no resources sees no resources section", async () =>
  andThen(
    shouldBeOk()(
      await handle(app(), getRequest("/")),
    ),
    (r) =>
      toBe(false)(
        htmlOf(r).includes("<h2>Resources</h2>"),
      ),
  ));

// ---- generic qfs browsing: any describable path, no per-resource code ----

// The shapes below were copied from real qfs 0.0.71 answers.
const GENERIC_DESCRIBE = {
  path: "/local/repo/docs",
  archetype: "blob_namespace",
  columns: [
    { name: "name", ty: "Text" },
    { name: "path", ty: "Text" },
  ],
};

const GENERIC_ANSWER = {
  schema: [
    { name: "name", type: "text" },
    { name: "path", type: "text" },
  ],
  rows: [
    {
      name: "index.md",
      path: "/local/repo/docs/index.md",
    },
    {
      name: "assets",
      path: "/local/repo/docs/assets",
    },
  ],
  meta: { truncated: false },
};

// Generic browsing needs no declaration — the config below declares NOTHING,
// which is exactly the zero-config case the mission's demo starts from.
const withQfs = (
  answer: unknown = GENERIC_ANSWER,
  described: unknown = GENERIC_DESCRIBE,
) =>
  api(
    indexRef(scan(fakeFileSystem(CORPUS))),
    undefined,
    undefined,
    fakeRunner(answer, described),
  );

test("a qfs path renders as the default column view: describe header, typed columns, rows", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withQfs(),
        getRequest(
          "/resolve/qfs:/local/repo/docs",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        // the node as describe named it
        toBe(true)(
          htmlOf(r).includes("blob_namespace"),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "<th>name (text)</th>",
          ),
        ),
        // corpus + the qfs column
        toBe(2)(columnCount(htmlOf(r))),
      ]),
  ));

// A click is a segment appended: the contained row links to THIS trail plus
// its own path, so how you arrived stays readable.
test("a contained row links to the trail plus its own segment", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withQfs(),
        getRequest(
          "/resolve/qfs:/local/repo/docs",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/local/repo/docs,qfs:/local/repo/docs/index.md"',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/local/repo/docs,qfs:/local/repo/docs/assets"',
          ),
        ),
      ]),
  ));

// qfs's own words for a path nothing serves — the describe error IS the
// column, so a typo'd path answers with the reason, not a blank.
test("a path qfs cannot describe shows qfs's refusal", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withQfs(GENERIC_ANSWER, {
          error: {
            code: "unknown_mount",
            message:
              "no driver is mounted for `/nosuch` (describe registry)",
          },
        }),
        getRequest("/resolve/qfs:/nosuch"),
      ),
    ),
    (r) =>
      toBe(true)(
        htmlOf(r).includes(
          "no driver is mounted",
        ),
      ),
  ));

// The door: the corpus column offers the path form wherever the server can
// reach qfs — and the form is a GET to /qfs, so the result is an address.
test("the corpus column offers the qfs path form", async () =>
  andThen(
    shouldBeOk()(
      await handle(withQfs(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes('action="/qfs"'),
        ),
        toBe(true)(
          htmlOf(r).includes('name="path"'),
        ),
      ]),
  ));

// A document and a walked qfs path in one trail — generic browsing joins the
// corpus rather than replacing it.
test("a document and a qfs path open side by side in one trail", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withQfs(),
        getRequest(
          "/resolve/docs/adr/index.md,qfs:/local/repo/docs",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(3)(columnCount(htmlOf(r))),
        toBe(true)(
          htmlOf(r).includes('<h1 id="adrs">'),
        ),
        toBe(true)(
          htmlOf(r).includes("blob_namespace"),
        ),
      ]),
  ));

// describe answered, the read did not — the column keeps the node's identity
// (path, archetype) and shows the read's failure in qfs's words.
test("a describable path whose read fails shows the archetype and the failure", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withQfs(
          {
            error: {
              code: "capability",
              message:
                "select is not supported here",
            },
          },
          GENERIC_DESCRIBE,
        ),
        getRequest(
          "/resolve/qfs:/local/repo/docs",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("blob_namespace"),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "select is not supported here",
          ),
        ),
      ]),
  ));

// The same honesty rule the declared-resource column follows.
test("a truncated generic read says so", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withQfs({
          ...GENERIC_ANSWER,
          meta: { truncated: true },
        }),
        getRequest(
          "/resolve/qfs:/local/repo/docs",
        ),
      ),
    ),
    (r) =>
      toBe(true)(
        htmlOf(r).includes(
          "you are not seeing every row",
        ),
      ),
  ));

// ---- the /resolve address subsumes ?cols= (docs/adr/0007) ----

// The legacy spelling MOVED: one serialization stays in circulation, and an
// aged bookmark is walked to it — filters and all — rather than answered by
// a second renderer that could drift from the first.
test("a legacy ?cols= address redirects permanently to its /resolve address", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getWithQuery("/", {
          cols: "docs/adr/index.md",
          type: "adr",
        }),
      ),
    ),
    (r) =>
      all([
        toBe(308)(statusOfResponse(r)),
        toBe(
          "/resolve/docs/adr/index.md?type=adr",
        )(r.headers["location"] ?? ""),
        // the redirect leaves through noStore like every other response
        toBe("no-store, must-revalidate")(
          r.headers["cache-control"],
        ),
      ]),
  ));

// The empty trail has ONE spelling, `/` — a /resolve address whose every
// segment dropped out redirects there instead of becoming a second root.
test("a /resolve address with nothing left redirects to the root", async () => {
  const bare = await handle(
    app(),
    getRequest("/resolve/notes.txt"),
  );
  const withFilter = await handle(
    app(),
    getWithQuery("/resolve/notes.txt", {
      type: "adr",
    }),
  );
  return andThen(shouldBeOk()(bare), (b) =>
    andThen(shouldBeOk()(withFilter), (f) =>
      all([
        toBe(308)(statusOfResponse(b)),
        toBe("/")(b.headers["location"] ?? ""),
        // the filter is data, so the walk keeps it
        toBe("/?type=adr")(
          f.headers["location"] ?? "",
        ),
      ]),
    ),
  );
});

// Display state is provably absent from the address: the trail is a function
// of the PATH alone, so no query parameter — a hypothetical `sort`, `fold`
// or `highlight` — can change which columns the address names.
test("no query parameter participates in the trail the address names", async () => {
  const plain = await handle(
    app(),
    getRequest("/resolve/docs/adr/index.md"),
  );
  const decorated = await handle(
    app(),
    getWithQuery("/resolve/docs/adr/index.md", {
      sort: "name",
      fold: "2",
      highlight: "adr",
    }),
  );
  return andThen(shouldBeOk()(plain), (p) =>
    andThen(shouldBeOk()(decorated), (d) =>
      all([
        // the same columns, both times: corpus + the one document
        toBe(2)(columnCount(htmlOf(p))),
        toBe(2)(columnCount(htmlOf(d))),
        toBe(true)(
          htmlOf(p).includes('<h1 id="adrs">'),
        ),
        toBe(true)(
          htmlOf(d).includes('<h1 id="adrs">'),
        ),
      ]),
    ),
  );
});

// The strip is the ENGINE's (ADR 0002, second amendment): one engine row
// holds every column, each column carries the engine's sticky header whose
// title is the collapse link, and the rail above the strip is the engine's
// breadcrumb folded out of the ONE Scene the trail lowers to. The hand-built
// shell (`<section class="column">`, its h2 headers) is gone — this is the
// spec that keeps it gone.
test("the strip renders through the engine: row, sticky headers, breadcrumb rail", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest(
          "/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md",
        ),
      ),
    ),
    (r) =>
      all([
        // one engine row, engine columns inside it
        toBe(1)(
          (
            htmlOf(r).match(
              /class="pm-row[ "]/g,
            ) ?? []
          ).length,
        ),
        toBe(3)(columnCount(htmlOf(r))),
        // the deepest column's header title collapses back to the trail
        // WITHOUT this column — leaving is the same gesture as entering,
        // a link (the engine's colHead contract)
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/docs/adr/index.md" aria-label="Reset to docs/adr/0001-npm-only.md"',
          ),
        ),
        // the breadcrumb rail, folded by the engine's crumbsOf from the
        // lowered Scene: the intermediate crumb links to the address at
        // which its own column is the deepest
        toBe(true)(
          htmlOf(r).includes('class="pm-crumbs"'),
        ),
        // the hand-built shell stays retired
        toBe(false)(
          htmlOf(r).includes(
            '<section class="column',
          ),
        ),
      ]),
  ));

// ---- the collection corpus (docs/adr/0008) ----

// The fixtures speak the qfs markdown driver's own schema; the runner
// answers by STATEMENT and refuses one nobody scripted, so these specs pin
// exactly what the collection source may ask qfs.
const DOCS_STATEMENT =
  "/markdown/tree/documents |> limit 100000";
const LINKS_STATEMENT =
  "/markdown/tree/links |> limit 100000";

const COLLECTION_DOCUMENTS = {
  schema: [
    { name: "path", type: "text" },
    { name: "title", type: "text" },
    { name: "frontmatter", type: "json" },
  ],
  rows: [
    {
      path: "docs/a.md",
      title: "A",
      frontmatter: { type: "adr" },
    },
    {
      path: "docs/b.md",
      title: null,
      frontmatter: null,
    },
  ],
  meta: { row_count: 2, truncated: false },
};

const COLLECTION_LINKS = {
  schema: [
    { name: "source_doc", type: "text" },
    {
      name: "source_section_path",
      type: "array(text)",
    },
    { name: "target", type: "text" },
    { name: "target_doc", type: "text" },
    { name: "line", type: "int" },
  ],
  rows: [
    {
      source_doc: "docs/a.md",
      source_section_path: ["A", "Refs"],
      target: "b.md",
      target_doc: "docs/b.md",
      line: 5,
    },
    {
      source_doc: "docs/a.md",
      source_section_path: [],
      target: "https://example.com/x",
      target_doc: null,
      line: 9,
    },
    {
      source_doc: "docs/a.md",
      source_section_path: [],
      target: "../outside.md",
      target_doc: null,
      line: 11,
    },
  ],
  meta: { row_count: 3, truncated: false },
};

const statementRunner = (
  answers: Readonly<Record<string, unknown>>,
): ResourceRunner => ({
  run: (statement) => {
    const answer = answers[statement];
    return answer === undefined
      ? err(
          invalidError({
            message: `unscripted statement: ${statement}`,
          }),
        )
      : ok(answer);
  },
  describe: () =>
    err(
      invalidError({
        message: "the collection never describes",
      }),
    ),
});

const collectionApp = (
  answers: Readonly<Record<string, unknown>> = {
    [DOCS_STATEMENT]: COLLECTION_DOCUMENTS,
    [LINKS_STATEMENT]: COLLECTION_LINKS,
  },
) => {
  const c = asConfig({ collection: "tree" });
  if (c.__tag === "Err") {
    throw new Error(c.content.content.message);
  }
  const runner = statementRunner(answers);
  return api(
    collectionRef(
      runner,
      fakeFileSystem({
        "docs/a.md": "# a body",
        "docs/b.md": "# b body",
        // On disk but not in the table: the walk would have served it,
        // the collection must not.
        "docs/unlisted.md": "# not listed",
      }),
      "tree",
    ),
    undefined,
    c.content,
    runner,
  );
};

// The acceptance's first clause: `/` browses documents — enumerated by the
// documents table and faceted by ITS frontmatter column, with the
// in-process scanner nowhere in the wiring.
test("GET / lists and facets the corpus from the documents table", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        collectionApp(),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes("docs/a.md"),
        ),
        toBe(true)(
          htmlOf(r).includes("docs/b.md"),
        ),
        // enumeration is qfs's alone
        toBe(false)(
          htmlOf(r).includes("docs/unlisted.md"),
        ),
        // the facet reads the TABLE's frontmatter column
        toBe(true)(htmlOf(r).includes("adr (1)")),
      ]),
  ));

// The acceptance's second clause, the sideways walk: the document column
// shows the links table's rows for that document, and an internal link is
// a strip segment — target document, one column to the right.
test("a document column walks sideways through the links table", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        collectionApp(),
        getRequest("/resolve/docs/a.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("<h2>Links</h2>"),
        ),
        // document → its links → target document
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/docs/a.md,docs/b.md"',
          ),
        ),
        // the section context the driver preserved, made visible
        toBe(true)(
          htmlOf(r).includes("A › Refs — "),
        ),
        // where the link was written, on the link itself
        toBe(true)(
          htmlOf(r).includes(
            'title="docs/a.md:5"',
          ),
        ),
        // an external target is an anchor to where it says
        toBe(true)(
          htmlOf(r).includes(
            'href="https://example.com/x"',
          ),
        ),
        // a root-escaping target is inert text — the corpus cannot open it
        toBe(true)(
          htmlOf(r).includes("../outside.md"),
        ),
        toBe(false)(
          htmlOf(r).includes(
            'href="../outside.md"',
          ),
        ),
      ]),
  ));

// No collection, no links table: the legacy corpus renders no Links
// section rather than asking qfs for a table that was never bound.
test("the legacy corpus renders no links section", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest("/resolve/docs/adr/index.md"),
      ),
    ),
    (r) =>
      toBe(false)(
        htmlOf(r).includes("<h2>Links</h2>"),
      ),
  ));

// A links table qfs cannot answer is qfs's own words under the body — the
// document still renders, because the body was never the table's to hold.
test("a refused links table is reported on the column, body intact", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        collectionApp({
          [DOCS_STATEMENT]: COLLECTION_DOCUMENTS,
        }),
        getRequest("/resolve/docs/a.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(htmlOf(r).includes("a body")),
        toBe(true)(
          htmlOf(r).includes(
            "unscripted statement",
          ),
        ),
      ]),
  ));

// A qfs that cannot serve the documents table is an EMPTY corpus that says
// why — the server stays up and the corpus column carries qfs's words.
test("a refused documents table is an empty corpus naming the failure", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        collectionApp({}),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            "/markdown/tree/documents",
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            "unscripted statement",
          ),
        ),
      ]),
  ));

// ---------------------------------------------------------------------------
// The root column's TWO AXES — derived from what qfs declares, never held.
//
// The developer's correction, and the substance of the ticket: "実際に github
// に接続して querying する際のパスと、driver 一覧があって、そのドライバーに
// どんなコネクションがあるか、というのは別のはずです". Two axes, not two
// views of one list.
//
// Every fixture below is INVENTED. `/sys/paths` and `/sys/connections` return
// the operator's real accounts and connection names on any machine that has
// connected anything, and none of that belongs in a repository. What is copied
// from the live probe is the column SHAPE and the driver vocabulary — facts
// about the qfs binary, not about anyone's mailbox.

const PATHS_ANSWER = {
  schema: [
    { name: "path", type: "text" },
    { name: "driver", type: "text" },
    { name: "alias_of", type: "text" },
    { name: "account", type: "text" },
  ],
  rows: [
    {
      path: "/team-mail",
      driver: "gmail",
      alias_of: "",
      account: "acme",
    },
    {
      path: "/team-files",
      driver: "gdrive",
      alias_of: "",
      account: "acme",
    },
  ],
  meta: { truncated: false },
};

const CONNECTIONS_ANSWER = {
  schema: [
    { name: "driver", type: "text" },
    { name: "connection", type: "text" },
  ],
  rows: [
    { driver: "google", connection: "acme" },
    {
      driver: "google",
      connection: "acme-backup",
    },
  ],
  meta: { truncated: false },
};

const EMPTY_ANSWER = {
  schema: [{ name: "path", type: "text" }],
  rows: [],
  meta: { truncated: false },
};

// A runner that answers each axis ITS OWN question. A fake that could not tell
// the two statements apart would prove nothing about keeping the axes
// separate, which is the one thing these tests exist to check.
const axisRunner = (
  paths: unknown = PATHS_ANSWER,
  connections: unknown = CONNECTIONS_ANSWER,
): ResourceRunner => ({
  run: (statement) =>
    statement.includes("/sys/paths")
      ? ok(paths)
      : statement.includes("/sys/connections")
        ? ok(connections)
        : err(
            invalidError({
              message:
                "the root column asked something other than the two axes",
            }),
          ),
  describe: () =>
    err(
      invalidError({
        message: "no describe in this fake",
      }),
    ),
});

const withAxes = (
  paths: unknown = PATHS_ANSWER,
  connections: unknown = CONNECTIONS_ANSWER,
) =>
  api(
    indexRef(scan(fakeFileSystem(CORPUS))),
    undefined,
    undefined,
    axisRunner(paths, connections),
  );

// AXIS 1: the paths you actually query. They NAVIGATE — each opens the path as
// a qfs column through the trail, which is the machinery that already exists.
test("the root column derives its query paths from what qfs declares", async () =>
  andThen(
    shouldBeOk()(
      await handle(withAxes(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("<h2>Paths</h2>"),
        ),
        // qfs said it, so the column shows it — and it is a link into the
        // strip, not decoration
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/team-mail"',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/team-files"',
          ),
        ),
      ]),
  ));

// AXIS 2: which drivers exist, and what connections each has. A VIEW, not
// navigation.
test("the root column derives the driver view from what qfs declares", async () =>
  andThen(
    shouldBeOk()(
      await handle(withAxes(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes("<h2>Drivers</h2>"),
        ),
        // the driver, and the connections grouped under it
        toBe(true)(htmlOf(r).includes("google")),
        toBe(true)(
          htmlOf(r).includes("acme, acme-backup"),
        ),
      ]),
  ));

// THE separation, asserted rather than asserted-about. `google` is a
// connection driver; `gmail` is a path driver; qfs never says they are the
// same thing, so neither does this column. A connection is not a link, because
// there is no path it could unambiguously mean — one `google` connection backs
// both `/team-mail` (gmail) and `/team-files` (gdrive).
test("the two axes stay separate: a connection is never rendered as a path link", async () =>
  andThen(
    shouldBeOk()(
      await handle(withAxes(), getRequest("/")),
    ),
    (r) =>
      all([
        // no trail segment was invented for a connection or a driver name
        toBe(false)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/google"',
          ),
        ),
        toBe(false)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/acme"',
          ),
        ),
        toBe(false)(
          htmlOf(r).includes(
            'href="/resolve/qfs:/gmail"',
          ),
        ),
      ]),
  ));

// "Nothing renders for anything qfs does not declare — not an empty menu, not
// a disabled one." A machine with nothing connected gets no sections at all.
test("an axis qfs declares nothing for renders nothing — not an empty menu", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        withAxes(EMPTY_ANSWER, EMPTY_ANSWER),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toBe(false)(
          htmlOf(r).includes("<h2>Paths</h2>"),
        ),
        toBe(false)(
          htmlOf(r).includes("<h2>Drivers</h2>"),
        ),
        // and the corpus is untouched by qfs having nothing to say —
        // "Markdown browsing does not need qfs" (Connection.ts)
        toBe(true)(
          htmlOf(r).includes(
            "docs/adr/0001-npm-only.md",
          ),
        ),
      ]),
  ));

// The distinction the union exists for: "qfs could not be asked" is NOT "qfs
// declares nothing". Collapsing them would report a broken qfs as a bare
// machine.
test("an axis qfs could not answer says so, in qfs's own words", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        api(
          indexRef(scan(fakeFileSystem(CORPUS))),
        ),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        // a server built with no runner cannot reach qfs, and says that
        // rather than implying nothing is connected
        toBe(true)(
          htmlOf(r).includes(
            "this server was built without a qfs runner",
          ),
        ),
        // still a page, and still the corpus: qfs is not required to read
        // markdown
        toBe(true)(
          htmlOf(r).includes(
            "docs/adr/0001-npm-only.md",
          ),
        ),
      ]),
  ));

// No frequently-used-path list, and no query-execution link: the developer
// scoped both as 将来あり得る, and neither ships.
test("no frequently-used ranking and no query-execution link ship in the root column", async () =>
  andThen(
    shouldBeOk()(
      await handle(withAxes(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(false)(
          htmlOf(r)
            .toLowerCase()
            .includes("frequently"),
        ),
        toBe(false)(
          htmlOf(r)
            .toLowerCase()
            .includes("recent path"),
        ),
        // the paths axis links to a path's COLUMN, never to a statement the
        // reader did not write
        toBe(false)(
          htmlOf(r).includes("|&gt; limit"),
        ),
        toBe(false)(
          htmlOf(r).includes("|> limit"),
        ),
      ]),
  ));

// ---------------------------------------------------------------------------
// The landmark contract (docs/adr/0010, D2) — THE machine check.
//
// This is the conformance check the "follow the reference" ticket asked for,
// and it deliberately asserts OUR rule rather than diffing our DOM against
// the reference's. The reference is kind-driven at every position, which
// emits no `main` at all whenever nothing is drilled into; we settled that
// as a defect to push upstream, not a rule to copy (ADR 0010). A probe that
// diffed the two trees would therefore have to go red on the one divergence
// we chose on purpose — it would be measuring our disagreement, not our
// conformance.
//
// What it CANNOT see, recorded so nobody mistakes green for conformance:
// D1 (we re-implement `multiColumn` rather than call it) and D3 (no
// declare/schedule layer) are CALL-STRUCTURE facts. Every assertion here is
// over rendered HTML, and a hand-rolled renderer that emits the same markup
// passes them all. D1/D3 are pinned by ADR 0010's prose and by the source
// grep in the ticket's Verification Method, not by this suite.
const landmarkCount = (
  html: string,
  tag: string,
): number =>
  (
    html.match(new RegExp(`<${tag}[ >]`, "g")) ??
    []
  ).length;

// Rule 1: the deepest column is the page's `main`. At depth 0 that is the
// corpus itself — the case the old `deepest ? mainPane : asidePane` rule
// missed entirely, because the corpus column bypassed it and hardcoded
// `navPane`. The root page rendered ZERO `main` landmarks.
test("the root page has exactly one main landmark", async () =>
  andThen(
    shouldBeOk()(
      await handle(app(), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(1)(landmarkCount(htmlOf(r), "main")),
        // nothing is open, so nothing has stepped back to `nav` yet
        toBe(0)(landmarkCount(htmlOf(r), "nav")),
      ]),
  ));

// The same rule, against the root column's OTHER shape. The test above runs
// with no qfs runner, so both axes render their "cannot be asked" note; this
// one runs with both axes DECLARED, which is the shape a real `npx qfs-viewer`
// on a connected machine renders — more sections, more headings, more links.
//
// Without this, the landmark check would only ever see the emptiest version of
// the column it is guarding, and "exactly one main" would be a claim about a
// page nobody visits. The sections are `section`, never `nav`: a second `nav`
// here would be a second landmark competing with the corpus's own role.
test("the landmark rule survives a root column with both axes declared", async () =>
  andThen(
    shouldBeOk()(
      await handle(withAxes(), getRequest("/")),
    ),
    (r) =>
      all([
        // the axes really are on this page — otherwise the assertions below
        // would be measuring the empty column all over again
        toBe(true)(
          htmlOf(r).includes("<h2>Paths</h2>"),
        ),
        toBe(true)(
          htmlOf(r).includes("<h2>Drivers</h2>"),
        ),
        toBe(1)(landmarkCount(htmlOf(r), "main")),
        toBe(0)(landmarkCount(htmlOf(r), "nav")),
        // still ONE column: the axes are content of the root column, not
        // columns of their own
        toBe(1)(columnCount(htmlOf(r))),
      ]),
  ));

// Rule 2: once something IS open, the corpus steps back to the role its
// MenuLevel kind declares. The deepest column takes the `main`.
test("opening a document moves main to it and returns the corpus to nav", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest("/resolve/docs/adr/index.md"),
      ),
    ),
    (r) =>
      all([
        toBe(1)(landmarkCount(htmlOf(r), "main")),
        toBe(1)(landmarkCount(htmlOf(r), "nav")),
      ]),
  ));

// The invariant that matters: EXACTLY ONE `main`, at every depth. Two would
// be as wrong as none — a page's primary content is one region.
test("exactly one main landmark survives at every depth", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        app(),
        getRequest(
          "/resolve/docs/adr/index.md,docs/adr/0001-npm-only.md",
        ),
      ),
    ),
    (r) =>
      all([
        // three columns: corpus + two documents
        toBe(3)(columnCount(htmlOf(r))),
        toBe(1)(landmarkCount(htmlOf(r), "main")),
        // the corpus is the menu -> `nav`
        toBe(1)(landmarkCount(htmlOf(r), "nav")),
        // the document left behind is kind-driven `complementary`, which is
        // the reference's own rule for a non-deepest level
        toBe(1)(
          landmarkCount(htmlOf(r), "aside"),
        ),
      ]),
  ));
