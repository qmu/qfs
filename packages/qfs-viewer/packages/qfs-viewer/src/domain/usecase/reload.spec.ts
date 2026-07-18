import {
  test,
  check,
  all,
  toBe,
  toEqual,
  toHaveLength,
} from "plgg-test";
import { isOk } from "plgg";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { fakeTimer } from "#qfs-viewer/testkit/fakeTimer";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import {
  applyChange,
  indexRef,
  debouncedReload,
} from "#qfs-viewer/domain/usecase/reload";
import {
  type Index,
  documentCount,
  documentSource,
  indexErrors,
  newErrors,
  listDocuments,
} from "#qfs-viewer/domain/model/Index";
import {
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

const sourceAt = (
  index: Index,
  path: string,
): string | undefined => {
  const p = asDocumentPath(path);
  if (!isOk(p)) {
    return undefined;
  }
  const s = documentSource(index, p.content);
  return s.__tag === "Some"
    ? s.content
    : undefined;
};

const pathsOf = (index: Index) =>
  listDocuments(index).map((d) =>
    documentPathString(d.content.path),
  );

test("applyChange re-reads a changed document into a NEW index, leaving the old one untouched", () => {
  // The invariant every surface depends on: an index is a value. A reader
  // holding `before` across a reload must still see the old source.
  const fs = fakeFileSystem({
    "docs/a.md": "# original",
  });
  const before = scan(fs);
  const editedFs = fakeFileSystem({
    "docs/a.md": "# edited",
  });
  const after = applyChange(
    editedFs,
    before,
    "docs/a.md",
    "changed",
  );
  return all([
    // the new value reflects the edit
    check(
      sourceAt(after, "docs/a.md"),
      toBe("# edited"),
    ),
    // the OLD value is unchanged — this is the whole point
    check(
      sourceAt(before, "docs/a.md"),
      toBe("# original"),
    ),
    // and they really are different values
    check(before === after, toBe(false)),
  ]);
});

test("applyChange adds a newly created document", () => {
  const before = scan(
    fakeFileSystem({ "docs/a.md": "# a" }),
  );
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md": "# a",
      "docs/new.md": "# new",
    }),
    before,
    "docs/new.md",
    "changed",
  );
  return all([
    check(documentCount(before), toBe(1)),
    check(documentCount(after), toBe(2)),
    check(
      sourceAt(after, "docs/new.md"),
      toBe("# new"),
    ),
  ]);
});

test("applyChange removes a deleted document, and the old index still has it", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md": "# a",
      "docs/b.md": "# b",
    }),
  );
  const after = applyChange(
    fakeFileSystem({ "docs/a.md": "# a" }),
    before,
    "docs/b.md",
    "removed",
  );
  return all([
    check(documentCount(after), toBe(1)),
    check(pathsOf(after), toEqual(["docs/a.md"])),
    check(documentCount(before), toBe(2)),
  ]);
});

test("a changed file that cannot be read is treated as removed, not as a crash", () => {
  // An editor's atomic-rename save briefly makes the old path unreadable. A
  // file that is not there is not in the corpus — that is the honest reading.
  const before = scan(
    fakeFileSystem({ "docs/a.md": "# a" }),
  );
  const after = applyChange(
    fakeFileSystem(
      { "docs/a.md": "# a" },
      new Set(["docs/a.md"]),
    ),
    before,
    "docs/a.md",
    "changed",
  );
  return check(documentCount(after), toBe(0));
});

test("a change inside a pruned directory never enters the corpus", () => {
  // The reload path has no walk to prune, and node_modules/plgg/README.md is
  // a perfectly valid DocumentPath — so without an explicit rule here, an
  // `npm install` on a served tree would inject every dependency's README
  // into the corpus, one watch event at a time. The walk's guarantee ("a
  // dependency's README is not this corpus") has to hold on both paths.
  const fs = fakeFileSystem({
    "docs/a.md": "# a",
    "node_modules/plgg/README.md": "# theirs",
    "packages/x/node_modules/dep/README.md":
      "# theirs too",
    "dist/out.md": "# built",
    ".git/COMMIT_EDITMSG.md": "# git",
  });
  const before = scan(fs);
  const after = [
    "node_modules/plgg/README.md",
    "packages/x/node_modules/dep/README.md",
    "dist/out.md",
    ".git/COMMIT_EDITMSG.md",
  ].reduce(
    (acc, p) =>
      applyChange(fs, acc, p, "changed"),
    before,
  );
  return all([
    check(documentCount(before), toBe(1)),
    // still just docs/a.md — none of the four leaked in
    check(documentCount(after), toBe(1)),
    check(pathsOf(after), toEqual(["docs/a.md"])),
  ]);
});

test("a change to a non-document path leaves the corpus alone", () => {
  const before = scan(
    fakeFileSystem({ "docs/a.md": "# a" }),
  );
  return all([
    check(
      documentCount(
        applyChange(
          fakeFileSystem({ "docs/a.md": "# a" }),
          before,
          "packages/x/src/main.ts",
          "changed",
        ),
      ),
      toBe(1),
    ),
    check(
      documentCount(
        applyChange(
          fakeFileSystem({ "docs/a.md": "# a" }),
          before,
          "/absolute/escape.md",
          "changed",
        ),
      ),
      toBe(1),
    ),
  ]);
});

test("indexRef hands out an immutable value: a reader across a swap sees a consistent index", () => {
  // The concurrency property, stated as data. A reader calls current() once
  // and keeps the VALUE; a reload swaps in a new one. The reader's index
  // cannot change underneath it, however long it takes — no lock needed.
  const ref = indexRef(
    scan(fakeFileSystem({ "docs/a.md": "# v1" })),
  );
  const readerHeld = ref.current();
  ref.swap(
    scan(fakeFileSystem({ "docs/a.md": "# v2" })),
  );
  return all([
    // the reader still sees the corpus as of when it started
    check(
      sourceAt(readerHeld, "docs/a.md"),
      toBe("# v1"),
    ),
    // a reader starting now sees the new one
    check(
      sourceAt(ref.current(), "docs/a.md"),
      toBe("# v2"),
    ),
  ]);
});

test("debouncedReload coalesces a burst into ONE swap", () => {
  // Editors do not emit one event per save, and a git checkout can touch
  // hundreds of files in milliseconds. Without coalescing the corpus churns.
  const timer = fakeTimer();
  const fs = fakeFileSystem({
    "docs/a.md": "# a2",
    "docs/b.md": "# b2",
  });
  const ref = indexRef(
    scan(
      fakeFileSystem({
        "docs/a.md": "# a1",
        "docs/b.md": "# b1",
      }),
    ),
  );
  let swaps = 0;
  const counting = {
    current: ref.current,
    swap: (next: Index) => {
      swaps += 1;
      ref.swap(next);
    },
  };
  const onChange = debouncedReload(
    fs,
    counting,
    timer,
    100,
  );

  onChange("docs/a.md", "changed");
  onChange("docs/b.md", "changed");
  onChange("docs/a.md", "changed");

  const beforeFlush = swaps;
  timer.run();

  return all([
    // nothing swapped while the window was open
    check(beforeFlush, toBe(0)),
    // one swap for the whole burst, not three
    check(swaps, toBe(1)),
    // and both edits landed
    check(
      sourceAt(ref.current(), "docs/a.md"),
      toBe("# a2"),
    ),
    check(
      sourceAt(ref.current(), "docs/b.md"),
      toBe("# b2"),
    ),
  ]);
});

test("debouncedReload lets the last change per path win", () => {
  // A file created then deleted inside one window is simply absent.
  const timer = fakeTimer();
  const ref = indexRef(
    scan(fakeFileSystem({ "docs/a.md": "# a" })),
  );
  const onChange = debouncedReload(
    fakeFileSystem({ "docs/a.md": "# a" }),
    ref,
    timer,
    100,
  );
  onChange("docs/a.md", "changed");
  onChange("docs/a.md", "removed");
  timer.run();
  return check(
    documentCount(ref.current()),
    toBe(0),
  );
});

// The index's `errors` describe the corpus as it is NOW, so a re-read has to
// be able to clear an error as well as raise one. Before errors were keyed by
// path, `withDocument` passed the whole list through unchanged: a fence the
// author had just fixed went on reporting forever.
test("fixing a declined front-matter block clears its error on reload", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
    }),
  );
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md":
        "---\nlayer: Config\n---\n# body",
    }),
    before,
    "docs/a.md",
    "changed",
  );
  return all([
    check(indexErrors(before), toHaveLength(1)),
    check(indexErrors(after), toHaveLength(0)),
    // the old value is untouched — the reload generated a new one
    check(documentCount(after), toBe(1)),
  ]);
});

test("breaking a front-matter block raises its error on reload, keeping the document", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nlayer: Config\n---\n# body",
    }),
  );
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
    }),
    before,
    "docs/a.md",
    "changed",
  );
  return all([
    check(indexErrors(before), toHaveLength(0)),
    check(indexErrors(after), toHaveLength(1)),
    check(documentCount(after), toBe(1)),
  ]);
});

test("deleting a document takes its collected error with it", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
      "docs/b.md": "# fine",
    }),
  );
  const after = applyChange(
    fakeFileSystem({ "docs/b.md": "# fine" }),
    before,
    "docs/a.md",
    "removed",
  );
  return all([
    check(indexErrors(before), toHaveLength(1)),
    check(indexErrors(after), toHaveLength(0)),
    check(documentCount(after), toBe(1)),
  ]);
});

// The reload log's rule. At boot every error gets a line; a reload only
// reported counts, so a fence that broke while the server ran named no file.
test("newErrors names the error a reload introduced", () => {
  const before = scan(
    fakeFileSystem({ "docs/a.md": "# fine" }),
  );
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
    }),
    before,
    "docs/a.md",
    "changed",
  );
  const fresh = newErrors(before, after);
  return all([
    check(fresh, toHaveLength(1)),
    check(
      fresh[0]?.content.path,
      toBe("docs/a.md"),
    ),
  ]);
});

test("newErrors stays quiet when nothing new broke", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
      "docs/b.md": "# fine",
    }),
  );
  // b changes and is still fine; a's error is carried over, not re-announced.
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
      "docs/b.md": "# edited",
    }),
    before,
    "docs/b.md",
    "changed",
  );
  return all([
    check(indexErrors(after), toHaveLength(1)),
    check(
      newErrors(before, after),
      toHaveLength(0),
    ),
  ]);
});

// A file can be fixed one way and broken another between reloads; that is a
// new fact, not the old one persisting.
test("newErrors reports a same-path error whose message changed", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
    }),
  );
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md": "---\nfoo: |\n---\n# body",
    }),
    before,
    "docs/a.md",
    "changed",
  );
  return all([
    check(
      newErrors(before, after),
      toHaveLength(1),
    ),
    check(
      newErrors(
        before,
        after,
      )[0]?.content.message.includes('"|"'),
      toBe(true),
    ),
  ]);
});

test("newErrors is silent when a reload only fixes things", () => {
  const before = scan(
    fakeFileSystem({
      "docs/a.md":
        "---\nfoo: &anchor bar\n---\n# body",
    }),
  );
  const after = applyChange(
    fakeFileSystem({
      "docs/a.md":
        "---\nlayer: Config\n---\n# body",
    }),
    before,
    "docs/a.md",
    "changed",
  );
  return check(
    newErrors(before, after),
    toHaveLength(0),
  );
});

// The infinite-reload bug, pinned. `swap` is where the server logs
// `index.reloaded`, so an unconditional swap made every watch event on any
// file announce a reload that never happened — and when the server's own log
// lived in the watched tree, the announcement caused the next event: 115
// reloads in four seconds, measured. plgg hit the same shape once already
// ("Stop the dev server reloading on its own build output").
test("a burst that touches no document does not swap at all", () => {
  const timer = fakeTimer();
  const files = { "docs/a.md": "# a" };
  const ref = indexRef(
    scan(fakeFileSystem(files)),
  );
  let swaps = 0;
  const counting = {
    current: ref.current,
    swap: (next: Index) => {
      swaps += 1;
      ref.swap(next);
    },
  };
  const onChange = debouncedReload(
    fakeFileSystem(files),
    counting,
    timer,
    100,
  );

  // Exactly the writes a served repository sees constantly: a source file, a
  // build artifact, the server's own log.
  onChange("src/thing.ts", "changed");
  onChange("log.ndjson", "changed");
  onChange("dist/bundle.js", "changed");
  onChange(
    "node_modules/plgg/README.md",
    "changed",
  );
  timer.run();

  return check(swaps, toBe(0));
});

test("a real document change still swaps, so the guard did not disable reload", () => {
  const timer = fakeTimer();
  const ref = indexRef(
    scan(fakeFileSystem({ "docs/a.md": "# v1" })),
  );
  let swaps = 0;
  const counting = {
    current: ref.current,
    swap: (next: Index) => {
      swaps += 1;
      ref.swap(next);
    },
  };
  const onChange = debouncedReload(
    fakeFileSystem({ "docs/a.md": "# v2" }),
    counting,
    timer,
    100,
  );

  // A non-document alongside a real edit must not mask the real one.
  onChange("log.ndjson", "changed");
  onChange("docs/a.md", "changed");
  timer.run();

  return all([
    check(swaps, toBe(1)),
    check(
      sourceAt(ref.current(), "docs/a.md"),
      toBe("# v2"),
    ),
  ]);
});
