import {
  test,
  check,
  all,
  toBe,
  toEqual,
  toHaveLength,
  shouldBeOk,
  shouldBeErr,
  andThen,
} from "plgg-test";
import { isOk } from "plgg";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import {
  scan,
  walkRoot,
  readDocument,
} from "#qfs-viewer/domain/usecase/scan";
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

const pathsOf = (
  index: ReturnType<typeof scan>,
) =>
  listDocuments(index).map((d) =>
    documentPathString(d.content.path),
  );

test("scan yields one document per markdown file, wherever it is scattered", () => {
  const index = scan(
    fakeFileSystem({
      ".workaholic/missions/index.md":
        "# missions",
      "docs/adr/0001-npm-only.md": "# 0001",
      "packages/qfs-viewer/README.md":
        "# qfs-viewer",
    }),
  );
  return all([
    check(documentCount(index), toBe(3)),
    check(
      pathsOf(index),
      toEqual([
        ".workaholic/missions/index.md",
        "docs/adr/0001-npm-only.md",
        "packages/qfs-viewer/README.md",
      ]),
    ),
  ]);
});

test("scan prunes node_modules and dist — a dependency's README is not this corpus", () => {
  // The load-bearing prune: `packages/` is a scan root, so one unpruned
  // node_modules would pull every dependency's markdown into the corpus.
  const index = scan(
    fakeFileSystem({
      "packages/qfs-viewer/README.md": "# ours",
      "packages/qfs-viewer/node_modules/plgg/README.md":
        "# theirs",
      "packages/qfs-viewer/dist/notes.md":
        "# built",
      "docs/.git/COMMIT_EDITMSG.md": "# git",
      "docs/outputs/run.md": "# output",
      "docs/coverage/report.md": "# coverage",
    }),
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(
      pathsOf(index),
      toEqual(["packages/qfs-viewer/README.md"]),
    ),
  ]);
});

// Found in the wild, not reasoned about: pointed at plgg, this tool reported
// 1714 documents of which 761 lived under `.worktrees/` — 44% of the corpus
// was the same knowledge at other commits, listed beside itself. A worktree is
// the METHOD in this house (every /trip makes one), so the tool duplicated
// exactly the repositories it exists for, and worst on the biggest.
test("scan prunes .worktrees — another branch's copy is not a second document", () => {
  const index = scan(
    fakeFileSystem({
      "docs/adr/0001.md": "# the live one",
      // The SAME document, as a worktree of another branch holds it. Without
      // the prune a reader searching for it gets it back twice, with no way to
      // tell which one is the branch they are on.
      ".worktrees/some-trip/docs/adr/0001.md":
        "# the same one, at another commit",
      ".worktrees/some-trip/README.md":
        "# also not ours to serve twice",
    }),
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(
      pathsOf(index),
      toEqual(["docs/adr/0001.md"]),
    ),
  ]);
});

test("scan takes markdown only, and never a dotfile", () => {
  const index = scan(
    fakeFileSystem({
      "docs/real.md": "# yes",
      "docs/logo.png": "binary",
      "docs/notes.txt": "text",
      "docs/.hidden.md": "# scratch",
      "docs/UPPER.MD": "# case-insensitive",
    }),
  );
  return all([
    check(documentCount(index), toBe(2)),
    check(
      pathsOf(index),
      toEqual(["docs/UPPER.MD", "docs/real.md"]),
    ),
  ]);
});

test("scan takes explicit roots — a docs site scans only what it declares", () => {
  // The roots are a parameter, not a constant: a plgg or qfs docs site built
  // on this mechanism scans its own tree, not this repository's convention.
  const fs = fakeFileSystem({
    "docs/a.md": "# a",
    "guide/b.md": "# b",
    "packages/c.md": "# c",
  });
  return all([
    check(
      pathsOf(scan(fs, ["guide"])),
      toEqual(["guide/b.md"]),
    ),
    check(
      pathsOf(scan(fs, ["docs", "guide"])),
      toEqual(["docs/a.md", "guide/b.md"]),
    ),
    // and the DEFAULT root is the whole tree: everything, minus the prunes.
    // An allowlist of directories is what left this repository's own
    // README.md out of its knowledge base.
    check(
      pathsOf(scan(fs)),
      toEqual([
        "docs/a.md",
        "guide/b.md",
        "packages/c.md",
      ]),
    ),
  ]);
});

test("the default scan reaches root-level files — a repo's README is knowledge", () => {
  // The regression this pins: the first cut hardcoded [.workaholic, docs,
  // packages], so README.md and CLAUDE.md — the two documents a reader looks
  // for first — were absent from the corpus entirely.
  const index = scan(
    fakeFileSystem({
      "README.md": "# readme",
      "CLAUDE.md": "# claude",
      "docs/a.md": "# a",
      "workloads/development/README.md":
        "# workload",
      ".workaholic/index.md": "# okf",
    }),
  );
  return all([
    check(documentCount(index), toBe(5)),
    check(
      pathsOf(index),
      toEqual([
        ".workaholic/index.md",
        "CLAUDE.md",
        "README.md",
        "docs/a.md",
        "workloads/development/README.md",
      ]),
    ),
  ]);
});

test("the default scan still prunes the noise at the tree root", () => {
  // Scanning "." is only safe because the prune list is: an unpruned
  // node_modules at the ROOT would pull in every dependency's markdown.
  const index = scan(
    fakeFileSystem({
      "README.md": "# ours",
      "node_modules/plgg/README.md": "# theirs",
      "dist/out.md": "# built",
      "coverage/report.md": "# coverage",
      "outputs/run.md": "# output",
      ".git/COMMIT_EDITMSG.md": "# git",
    }),
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(pathsOf(index), toEqual(["README.md"])),
  ]);
});

test("scan skips an absent root rather than failing — not every repo has docs/", () => {
  const index = scan(
    fakeFileSystem({ "docs/a.md": "# a" }),
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(indexErrors(index), toHaveLength(0)),
  ]);
});

test("scan is empty, not broken, on an empty corpus", () => {
  const index = scan(fakeFileSystem({}));
  return all([
    check(documentCount(index), toBe(0)),
    check(indexErrors(index), toHaveLength(0)),
  ]);
});

test("scan skips-and-collects an unreadable file: one bad file cannot take the index down", () => {
  // A file that vanishes between the walk and the read (an editor's atomic
  // rename), or that bites on permissions. The corpus is input, not
  // invariant — the other documents must still index.
  const index = scan(
    fakeFileSystem(
      {
        "docs/good.md": "# good",
        "docs/vanished.md": "# gone by read time",
        "docs/also-good.md": "# fine",
      },
      new Set(["docs/vanished.md"]),
    ),
  );
  return all([
    // the good documents are indexed
    check(documentCount(index), toBe(2)),
    check(
      pathsOf(index),
      toEqual([
        "docs/also-good.md",
        "docs/good.md",
      ]),
    ),
    // and the failure is reported, not swallowed
    check(indexErrors(index), toHaveLength(1)),
    check(
      indexErrors(index)[0]?.content.path,
      toBe("docs/vanished.md"),
    ),
  ]);
});

test("walkRoot survives a directory that cannot be statted: skipped, not fatal", () => {
  // The claim the walk makes in its comment, proven. A permission-denied
  // directory somewhere under packages/ must not end the walk and lose the
  // rest of the corpus.
  const fs = fakeFileSystem(
    {
      "docs/a.md": "# a",
      "docs/locked/secret.md": "# secret",
      "docs/z.md": "# z",
    },
    new Set(),
    new Set(["docs/locked"]),
  );
  return check(
    walkRoot(fs, "docs"),
    toEqual(["docs/a.md", "docs/z.md"]),
  );
});

test("walkRoot survives a directory that stats but cannot be listed", () => {
  // The other half: this directory answers isDirectory fine and only bites on
  // the listing call, so the walk fails over at a different point than the
  // un-stattable case above.
  const fs = fakeFileSystem(
    {
      "docs/a.md": "# a",
      "docs/locked/secret.md": "# secret",
      "docs/z.md": "# z",
    },
    new Set(),
    new Set(),
    new Set(["docs/locked"]),
  );
  return check(
    walkRoot(fs, "docs"),
    toEqual(["docs/a.md", "docs/z.md"]),
  );
});

test("walkRoot survives a hostile root: an unstattable root is empty, not fatal", () =>
  check(
    walkRoot(
      fakeFileSystem(
        { "docs/a.md": "# a" },
        new Set(),
        new Set(["docs"]),
      ),
      "docs",
    ),
    toEqual([]),
  ));

test("walkRoot descends depth-first and finds nested documents", () =>
  check(
    walkRoot(
      fakeFileSystem({
        "docs/a.md": "",
        "docs/deep/b.md": "",
        "docs/deep/deeper/c.md": "",
      }),
      "docs",
    ),
    toEqual([
      "docs/a.md",
      "docs/deep/b.md",
      "docs/deep/deeper/c.md",
    ]),
  ));

test("readDocument reads a document's source", () =>
  check(
    readDocument(
      fakeFileSystem({ "docs/a.md": "# hello" }),
      "docs/a.md",
    ),
    shouldBeOk(),
  ));

test("readDocument reports an unreadable file as a ScanError, never a throw", () =>
  andThen(
    shouldBeErr()(
      readDocument(
        fakeFileSystem(
          { "docs/a.md": "x" },
          new Set(["docs/a.md"]),
        ),
        "docs/a.md",
      ),
    ),
    (e) => toBe("docs/a.md")(e.content.path),
  ));

test("readDocument rejects a path that is not a valid document path", () =>
  check(
    readDocument(
      fakeFileSystem({}),
      "/absolute/escape.md",
    ),
    shouldBeErr(),
  ));

test("getDocument and documentSource are None for a path the corpus does not hold", () => {
  const index = scan(
    fakeFileSystem({ "docs/a.md": "# hello" }),
  );
  const absent = asDocumentPath("docs/absent.md");
  return isOk(absent)
    ? all([
        check(
          getDocument(index, absent.content)
            .__tag,
          toBe("None"),
        ),
        check(
          documentSource(index, absent.content)
            .__tag,
          toBe("None"),
        ),
      ])
    : check("unreachable", toBe("valid path"));
});

test("getDocument finds a scanned document by its path", () => {
  const index = scan(
    fakeFileSystem({ "docs/a.md": "# hello" }),
  );
  const path = asDocumentPath("docs/a.md");
  return isOk(path)
    ? check(
        getDocument(index, path.content),
        (o) =>
          toBe(true)(
            o.__tag === "Some" &&
              o.content.content.source ===
                "# hello",
          ),
      )
    : check(false, toBe(true));
});

test("scan projects a parsed front matter block into the document", () => {
  const index = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\ntitle: hello\n---\n# body",
    }),
  );
  return check(
    listDocuments(index)[0]?.content.frontMatter
      .__tag,
    toBe("Some"),
  );
});

test("a fence-less file indexes cleanly with no front matter and no error", () => {
  const index = scan(
    fakeFileSystem({
      "docs/a.md": "# just a body",
    }),
  );
  return all([
    check(
      listDocuments(index)[0]?.content.frontMatter
        .__tag,
      toBe("None"),
    ),
    check(indexErrors(index), toHaveLength(0)),
  ]);
});

// The decision this ticket turns on: a fence plgg-md will not parse must not
// cost the corpus its document. The body is perfectly readable; only the
// faceting head is unavailable.
//
// The fixture is `&anchor` rather than the `layer: [Config]` this was first
// written against, because 0.0.3 now ACCEPTS flow sequences — the upstream fix
// this project asked for, which correctly broke these tests. Alias expansion
// is excluded because it is genuine attack surface (a billion-laughs document
// is exactly what fail-closed exists to stop), so it will not be widened later
// and this fixture will not rot the same way twice.
test("a front-matter block the YAML subset declines still indexes the document", () => {
  const index = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
    }),
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(
      listDocuments(index)[0]?.content.frontMatter
        .__tag,
      toBe("None"),
    ),
  ]);
});

test("a declined front-matter block is reported as a collected error", () =>
  andThen(
    check(
      indexErrors(
        scan(
          fakeFileSystem({
            "docs/a.md":
              "---\nfoo: &anchor bar\n---\n# body",
          }),
        ),
      ),
      toHaveLength(1),
    ),
    () =>
      check(
        indexErrors(
          scan(
            fakeFileSystem({
              "docs/a.md":
                "---\nfoo: &anchor bar\n---\n# body",
            }),
          ),
        )[0]?.content.path,
        toBe("docs/a.md"),
      ),
  ));

test("an unterminated fence is reported but does not take the scan down", () => {
  const index = scan(
    fakeFileSystem({
      "docs/bad.md":
        "---\ntitle: x\nno closing fence",
      "docs/good.md": "# fine",
    }),
  );
  return all([
    check(documentCount(index), toBe(2)),
    check(indexErrors(index), toHaveLength(1)),
  ]);
});

test("readDocument surfaces a declined fence as a front-matter error beside the document", () =>
  andThen(
    shouldBeOk()(
      readDocument(
        fakeFileSystem({
          "docs/a.md": "---\nfoo: |\n---\n# body",
        }),
        "docs/a.md",
      ),
    ),
    (r) =>
      check(
        r.frontMatterError.__tag,
        toBe("Some"),
      ),
  ));

// The workaholic ticket format, indexed against the REAL plgg-md rather than a
// fake that agrees with us. Every field here is what `.workaholic/tickets/*`
// actually writes, and under plgg-md 0.0.2 this whole block was rejected: the
// single-line flow sequence and the empty values were both outside the subset.
// 0.0.3 accepts them — the upstream fix this project asked for — so this pins
// the corpus we exist to browse, and would go red if that regressed.
test("the workaholic ticket front-matter format facets cleanly", () => {
  const index = scan(
    fakeFileSystem({
      "t.md": [
        "---",
        "created_at: 2026-07-15T17:11:31+09:00",
        "author: a@qmu.jp",
        "type: housekeeping",
        "layer: [Config]",
        "effort:",
        "commit_hash:",
        "depends_on:",
        "mission: build-insightbrowser-on-the-plgg-family",
        "---",
        "# body",
      ].join("\n"),
    }),
  );
  return all([
    check(documentCount(index), toBe(1)),
    check(indexErrors(index), toHaveLength(0)),
    check(
      listDocuments(index)[0]?.content.frontMatter
        .__tag,
      toBe("Some"),
    ),
  ]);
});

// The subset is fail-closed BY DESIGN, and that half must not be widened by a
// future "make it accept more" change. These are genuine attack surface —
// alias expansion is a billion-laughs vector — so a green here is as
// load-bearing as the acceptance above.
test("the subset stays closed on the constructs that are attack surface", () =>
  all(
    [
      ["alias", "foo: &anchor bar"],
      ["tag", "foo: !!str bar"],
      ["block scalar", "foo: |"],
      ["folded scalar", "foo: >"],
    ].map(([, fm]) =>
      check(
        indexErrors(
          scan(
            fakeFileSystem({
              "t.md": `---\n${fm}\n---\n# body`,
            }),
          ),
        ),
        toHaveLength(1),
      ),
    ),
  ));
