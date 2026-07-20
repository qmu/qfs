// The collection corpus source (docs/adr/0008), proven over fakes: a fake
// runner answering the documents/links tables, a fake filesystem holding
// the bodies. The two proofs the ticket names are here — the table (not the
// fence) is the front-matter truth, and the scanner is INERT by
// construction — because they are the retirement's whole point.
import {
  test,
  check,
  all,
  toBe,
  toEqual,
  shouldBeOk,
  andThen,
} from "plgg-test";
import { ok, err, invalidError } from "plgg";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import {
  type FileSystem,
  type ResourceRunner,
} from "#qfs-viewer/domain/model/Scan";
import {
  collectionIndex,
  collectionRef,
  documentLinks,
} from "#qfs-viewer/domain/usecase/collection";
import {
  documentCount,
  indexErrors,
  listDocuments,
  getDocument,
  documentSource,
} from "#qfs-viewer/domain/model/Index";
import {
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

const documentsAnswer = (
  rows: ReadonlyArray<
    Readonly<Record<string, unknown>>
  >,
  truncated: boolean = false,
) => ({
  schema: [
    { name: "path", type: "text" },
    { name: "title", type: "text" },
    { name: "frontmatter", type: "json" },
  ],
  rows,
  meta: { row_count: rows.length, truncated },
});

const linksAnswer = (
  rows: ReadonlyArray<
    Readonly<Record<string, unknown>>
  >,
) => ({
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
  rows,
  meta: {
    row_count: rows.length,
    truncated: false,
  },
});

// A runner that answers each statement from a literal map — and REFUSES a
// statement nobody scripted, so a spec cannot quietly pass on a statement
// this usecase was never meant to issue.
const fakeRunner = (
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
        message:
          "the collection source never describes",
      }),
    ),
});

const DOCS_STATEMENT =
  "/markdown/tree/documents |> limit 100000";
const LINKS_STATEMENT =
  "/markdown/tree/links |> limit 100000";

// A filesystem that can ONLY point-read named files: its walk surface
// throws. This is "verifiably inert" made literal — if any collection read
// so much as stats a directory, the spec explodes.
const pointReadOnly = (
  files: Readonly<Record<string, string>>,
): FileSystem => {
  const readable = fakeFileSystem(files);
  return {
    readDirectory: () => {
      throw new Error(
        "the scanner walked: readDirectory must never be called on the collection arm",
      );
    },
    isDirectory: () => {
      throw new Error(
        "the scanner walked: isDirectory must never be called on the collection arm",
      );
    },
    readFile: readable.readFile,
  };
};

test("the corpus is enumerated by the documents table, bodies read by the paths it named", () => {
  const index = collectionIndex(
    fakeRunner({
      [DOCS_STATEMENT]: documentsAnswer([
        {
          path: "docs/a.md",
          title: "A",
          frontmatter: { type: "adr" },
        },
        {
          path: "README.md",
          title: null,
          frontmatter: null,
        },
      ]),
    }),
    pointReadOnly({
      "docs/a.md": "# a body",
      "README.md": "# readme",
      // On disk but NOT in the table: the walk would have found it, the
      // collection must not — enumeration is qfs's alone.
      "docs/unlisted.md": "# not in the table",
    }),
    "tree",
  );
  const unlisted = asDocumentPath(
    "docs/unlisted.md",
  );
  return all([
    check(documentCount(index), toBe(2)),
    check(indexErrors(index).length, toBe(0)),
    check(
      listDocuments(index).map((d) =>
        documentPathString(d.content.path),
      ),
      toEqual(["README.md", "docs/a.md"]),
    ),
    check(
      unlisted.__tag === "Ok"
        ? getDocument(index, unlisted.content)
            .__tag
        : "",
      toBe("None"),
    ),
  ]);
});

// THE parallel-truth proof: the file's own fence says one thing, the table
// says another, and the corpus facets on the TABLE — the in-process fence
// parse is not merely unused, there is nothing left that could disagree
// with qfs about what a document's front matter is.
test("the documents table is the front-matter truth, not the file's fence", () => {
  const index = collectionIndex(
    fakeRunner({
      [DOCS_STATEMENT]: documentsAnswer([
        {
          path: "docs/a.md",
          title: null,
          frontmatter: { type: "from-the-table" },
        },
      ]),
    }),
    pointReadOnly({
      "docs/a.md":
        "---\ntype: from-the-fence\n---\n# body",
    }),
    "tree",
  );
  const doc = listDocuments(index)[0];
  return check(
    doc !== undefined &&
      doc.content.frontMatter.__tag === "Some"
      ? doc.content.frontMatter.content
      : {},
    toEqual({ type: "from-the-table" }),
  );
});

// Skip-and-collect, exactly as the legacy scan: the table asserted a
// document the disk no longer backs (a race, an atomic-rename window), and
// that is a line in `errors`, not a crash and not a silent omission.
test("a listed document whose body cannot be read is a collected error", () => {
  const index = collectionIndex(
    fakeRunner({
      [DOCS_STATEMENT]: documentsAnswer([
        {
          path: "docs/gone.md",
          title: null,
          frontmatter: null,
        },
        {
          path: "docs/here.md",
          title: null,
          frontmatter: null,
        },
      ]),
    }),
    pointReadOnly({ "docs/here.md": "# here" }),
    "tree",
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(indexErrors(index).length, toBe(1)),
    check(
      indexErrors(index)[0]?.content.path,
      toBe("docs/gone.md"),
    ),
  ]);
});

// A qfs that cannot answer — not on PATH, no binding for the tree — is an
// EMPTY corpus carrying qfs's own words at the table's address. The server
// stays up; the corpus column says why it is empty.
test("a refused documents statement is an empty corpus with qfs's own words", () => {
  const index = collectionIndex(
    fakeRunner({}),
    pointReadOnly({}),
    "tree",
  );
  return all([
    check(documentCount(index), toBe(0)),
    check(indexErrors(index).length, toBe(1)),
    check(
      indexErrors(index)[0]?.content.path,
      toBe("/markdown/tree/documents"),
    ),
  ]);
});

test("a truncated listing says so beside the documents it did list", () => {
  const index = collectionIndex(
    fakeRunner({
      [DOCS_STATEMENT]: documentsAnswer(
        [
          {
            path: "docs/a.md",
            title: null,
            frontmatter: null,
          },
        ],
        true,
      ),
    }),
    pointReadOnly({ "docs/a.md": "# a" }),
    "tree",
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(
      indexErrors(index).some((e) =>
        e.content.message.includes("truncated"),
      ),
      toBe(true),
    ),
  ]);
});

// The ref's whole contract: every current() is a fresh read, and swap is
// discarded — installing a snapshot would start it aging immediately.
test("collectionRef reads afresh per current() and discards swaps", () => {
  const answers = new Map<string, unknown>([
    [
      DOCS_STATEMENT,
      documentsAnswer([
        {
          path: "docs/a.md",
          title: null,
          frontmatter: null,
        },
      ]),
    ],
  ]);
  const runner: ResourceRunner = {
    run: (statement) => {
      const answer = answers.get(statement);
      return answer === undefined
        ? err(invalidError({ message: "gone" }))
        : ok(answer);
    },
    describe: () =>
      err(invalidError({ message: "never" })),
  };
  const ref = collectionRef(
    runner,
    pointReadOnly({
      "docs/a.md": "# a",
      "docs/b.md": "# b",
    }),
    "tree",
  );
  const first = documentCount(ref.current());
  // The corpus changes UNDER the ref — as an edit landing between two
  // requests does — and the next read sees it with no swap and no watcher.
  answers.set(
    DOCS_STATEMENT,
    documentsAnswer([
      {
        path: "docs/a.md",
        title: null,
        frontmatter: null,
      },
      {
        path: "docs/b.md",
        title: null,
        frontmatter: null,
      },
    ]),
  );
  ref.swap(ref.current());
  const second = documentCount(ref.current());
  return all([
    check(first, toBe(1)),
    check(second, toBe(2)),
  ]);
});

// The body still serves through the same Index reads every surface makes.
test("a collection document's source answers like any other document's", () => {
  const index = collectionIndex(
    fakeRunner({
      [DOCS_STATEMENT]: documentsAnswer([
        {
          path: "docs/a.md",
          title: null,
          frontmatter: null,
        },
      ]),
    }),
    pointReadOnly({ "docs/a.md": "# the body" }),
    "tree",
  );
  const path = asDocumentPath("docs/a.md");
  const source =
    path.__tag === "Ok"
      ? documentSource(index, path.content)
      : undefined;
  return check(
    source !== undefined &&
      source.__tag === "Some"
      ? source.content
      : "",
    toBe("# the body"),
  );
});

// ---- the links of one document ----

test("documentLinks narrows the tree's links table to one source document", () =>
  andThen(
    shouldBeOk()(
      documentLinks(
        fakeRunner({
          [LINKS_STATEMENT]: linksAnswer([
            {
              source_doc: "docs/a.md",
              source_section_path: ["A"],
              target: "b.md",
              target_doc: "docs/b.md",
              line: 4,
            },
            {
              source_doc: "docs/other.md",
              source_section_path: [],
              target: "c.md",
              target_doc: "docs/c.md",
              line: 1,
            },
          ]),
        }),
        "tree",
        "docs/a.md",
      ),
    ),
    (links) =>
      all([
        check(links.length, toBe(1)),
        check(links[0]?.target, toBe("b.md")),
        check(
          links[0]?.targetDoc.__tag === "Some"
            ? links[0].targetDoc.content
            : "",
          toBe("docs/b.md"),
        ),
      ]),
  ));

test("a refused links statement is the error, in qfs's own words", () =>
  check(
    documentLinks(
      fakeRunner({}),
      "tree",
      "docs/a.md",
    ).__tag,
    toBe("Err"),
  ));
