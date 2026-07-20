// The collection path's reading of qfs's answers — the ticket's "unit specs
// against fixtures of the documents/links tables". The fixtures copy the
// shapes the qfs markdown driver's schema promises (path/title/frontmatter;
// source_doc/source_section_path/target/target_doc/line), because the
// boundary this module guards is exactly that promise.
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
  asCollectionName,
  asCollectionDocuments,
  asCollectionLinks,
  documentsStatement,
  linksStatement,
  documentsPath,
  linksPath,
  COLLECTION_ROW_LIMIT,
} from "#qfs-viewer/domain/model/Collection";
import { documentPathString } from "#qfs-viewer/domain/model/Vocabulary";

// A real documents answer's shape: one fenced document, one fence-less.
const DOCUMENTS_ANSWER = {
  schema: [
    { name: "path", type: "text" },
    { name: "title", type: "text" },
    { name: "frontmatter", type: "json" },
  ],
  rows: [
    {
      path: "docs/plan.md",
      title: "The plan",
      frontmatter: {
        type: "plan",
        layer: ["Domain", "UX"],
      },
    },
    {
      path: "README.md",
      title: null,
      frontmatter: null,
    },
  ],
  meta: { row_count: 2, truncated: false },
};

const LINKS_ANSWER = {
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
      source_doc: "docs/plan.md",
      source_section_path: ["The plan", "Steps"],
      target: "../README.md",
      target_doc: "README.md",
      line: 12,
    },
    {
      source_doc: "docs/plan.md",
      source_section_path: [],
      target: "https://example.com",
      target_doc: null,
      line: 3,
    },
  ],
  meta: { row_count: 2, truncated: false },
};

// ---- the tree name: the injection boundary ----

test("a tree name is one path segment from a closed charset", () =>
  all([
    check(
      asCollectionName("strategy").__tag,
      toBe("Ok"),
    ),
    check(
      asCollectionName("my-tree_0.1").__tag,
      toBe("Ok"),
    ),
  ]));

// The name is embedded verbatim in a statement, so everything a statement
// could be broken with — whitespace, quotes, pipes, slashes — must be
// unrepresentable, not merely unlikely.
test("a name that could reach the statement grammar is refused", () =>
  all([
    check(
      asCollectionName("a b").__tag,
      toBe("Err"),
    ),
    check(
      asCollectionName("a/b").__tag,
      toBe("Err"),
    ),
    check(
      asCollectionName("a|>rm").__tag,
      toBe("Err"),
    ),
    check(
      asCollectionName("a'b").__tag,
      toBe("Err"),
    ),
    check(asCollectionName("").__tag, toBe("Err")),
    check(asCollectionName(7).__tag, toBe("Err")),
  ]));

// ---- the statements: constants of the tree name ----

// The BARE /markdown spelling is qfs main's canonical address for this
// driver (docs/adr/0008 records the verification): qfs #7 retired bare
// paths for the HOSTS realm only, and qfs's generated drivers.md teaches
// /markdown/<name>/… as-is.
test("the statements address the canonical bare /markdown tables", () =>
  all([
    check(
      documentsPath("strategy"),
      toBe("/markdown/strategy/documents"),
    ),
    check(
      linksPath("strategy"),
      toBe("/markdown/strategy/links"),
    ),
    check(
      documentsStatement("strategy"),
      toBe(
        `/markdown/strategy/documents |> limit ${COLLECTION_ROW_LIMIT}`,
      ),
    ),
    check(
      linksStatement("strategy"),
      toBe(
        `/markdown/strategy/links |> limit ${COLLECTION_ROW_LIMIT}`,
      ),
    ),
  ]));

// ---- the documents table ----

test("a documents answer reads into typed rows, NULL frontmatter as None", () =>
  andThen(
    shouldBeOk()(
      asCollectionDocuments(DOCUMENTS_ANSWER),
    ),
    (read) =>
      all([
        check(read.documents.length, toBe(2)),
        check(read.errors.length, toBe(0)),
        check(read.truncated, toBe(false)),
        check(
          read.documents[0] === undefined
            ? ""
            : documentPathString(
                read.documents[0].path,
              ),
          toBe("docs/plan.md"),
        ),
        check(
          read.documents[0]?.frontMatter.__tag,
          toBe("Some"),
        ),
        check(
          read.documents[1]?.frontMatter.__tag,
          toBe("None"),
        ),
      ]),
  ));

// The frontmatter column IS the interpretation — carried as the plain data
// qfs reported, arrays and all, with no re-parse on this side.
test("the frontmatter column arrives as the fields qfs parsed", () =>
  andThen(
    shouldBeOk()(
      asCollectionDocuments(DOCUMENTS_ANSWER),
    ),
    (read) =>
      check(
        read.documents[0]?.frontMatter.__tag ===
          "Some"
          ? read.documents[0].frontMatter.content
          : {},
        toEqual({
          type: "plan",
          layer: ["Domain", "UX"],
        }),
      ),
  ));

// A row is another program's assertion, and the path locks still apply: a
// traversing or absolute path must land in `errors`, never on a filesystem.
test("an unacceptable path row is collected, not served and not fatal", () =>
  andThen(
    shouldBeOk()(
      asCollectionDocuments({
        ...DOCUMENTS_ANSWER,
        rows: [
          ...DOCUMENTS_ANSWER.rows,
          {
            path: "../escape.md",
            title: null,
            frontmatter: null,
          },
        ],
      }),
    ),
    (read) =>
      all([
        check(read.documents.length, toBe(2)),
        check(read.errors.length, toBe(1)),
        check(
          read.errors[0]?.content.path,
          toBe("../escape.md"),
        ),
      ]),
  ));

// A scalar where the schema promises an object is qfs breaking its own
// contract — said out loud beside the document list, not faceted on.
test("a non-object frontmatter value is a collected error", () =>
  andThen(
    shouldBeOk()(
      asCollectionDocuments({
        ...DOCUMENTS_ANSWER,
        rows: [
          {
            path: "docs/odd.md",
            title: null,
            frontmatter: "scalar",
          },
        ],
      }),
    ),
    (read) =>
      all([
        check(read.documents.length, toBe(0)),
        check(read.errors.length, toBe(1)),
      ]),
  ));

test("truncation of the listing is carried, because the corpus would lie", () =>
  andThen(
    shouldBeOk()(
      asCollectionDocuments({
        ...DOCUMENTS_ANSWER,
        meta: { truncated: true },
      }),
    ),
    (read) => check(read.truncated, toBe(true)),
  ));

// qfs's own error object stays qfs's own words — the person who declared
// the collection reads them.
test("a qfs error answer is reported as itself", () =>
  andThen(
    shouldBeErr()(
      asCollectionDocuments({
        error: {
          code: "unknown_mount",
          message:
            "no driver is mounted for /markdown/nope",
        },
      }),
    ),
    (e) =>
      check(
        e.content.message.includes(
          "unknown_mount",
        ),
        toBe(true),
      ),
  ));

// ---- the links table ----

test("a links answer reads into typed links, section path and all", () =>
  andThen(
    shouldBeOk()(asCollectionLinks(LINKS_ANSWER)),
    (links) =>
      all([
        check(links.length, toBe(2)),
        check(
          links[0]?.sectionPath,
          toEqual(["The plan", "Steps"]),
        ),
        check(
          links[0]?.target,
          toBe("../README.md"),
        ),
        check(
          links[0]?.targetDoc.__tag === "Some"
            ? links[0].targetDoc.content
            : "",
          toBe("README.md"),
        ),
        check(links[0]?.line, toBe(12)),
        // NULL target_doc — an external target — is None, not "".
        check(
          links[1]?.targetDoc.__tag,
          toBe("None"),
        ),
        check(links[1]?.sectionPath, toEqual([])),
      ]),
  ));

// A malformed row drops out; a link is a decoration on a document column,
// and the document itself is not what the row is about.
test("a links row the schema does not promise is skipped", () =>
  andThen(
    shouldBeOk()(
      asCollectionLinks({
        ...LINKS_ANSWER,
        rows: [
          ...LINKS_ANSWER.rows,
          {
            source_doc: "docs/plan.md",
            source_section_path: "not-an-array",
            target: "x.md",
            target_doc: null,
            line: 1,
          },
        ],
      }),
    ),
    (links) => check(links.length, toBe(2)),
  ));

test("a qfs error on the links table is reported as itself", () =>
  check(
    asCollectionLinks({
      error: { message: "boom" },
    }).__tag,
    toBe("Err"),
  ));
