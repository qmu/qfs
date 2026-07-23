// Editing, driven through the real router.
//
// The writer is a fake that records what it was asked to write, so these
// assert what reaches the disk WITHOUT touching one — and the read side is the
// same `fakeFileSystem` everything else uses.
import {
  test,
  check,
  all,
  toBe,
  toEqual,
  shouldBeOk,
  andThen,
} from "plgg-test";
import {
  handle,
  getRequest,
  type ResponseBody,
} from "plgg-server";
import { none } from "plgg";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import { indexRef } from "#qfs-viewer/domain/usecase/reload";
import { api } from "#qfs-viewer/entrypoints/api";
import { type FileWriter } from "#qfs-viewer/domain/model/Editor";
import { documentPathString } from "#qfs-viewer/domain/model/Vocabulary";
import { asConfig } from "#qfs-viewer/domain/model/Config";

const CORPUS = {
  "docs/a.md": "---\ntype: note\n---\n# before\n",
};

// Records the writes and, crucially, ALSO applies them to the tree the read
// seam sees — otherwise the "index updated now" assertion would pass against a
// filesystem that never changed, and prove nothing.
const recordingWorld = (
  files: Readonly<
    Record<string, string>
  > = CORPUS,
) => {
  const tree: Record<string, string> = {
    ...files,
  };
  const writes: Array<[string, string]> = [];
  const writer: FileWriter = {
    writeFile: (path, source) => {
      const key = documentPathString(path);
      writes.push([key, source]);
      tree[key] = source;
    },
  };
  // The fake reads `tree` live, so a write is visible to the next read.
  const fs = {
    readDirectory: (dir: string) =>
      fakeFileSystem(tree).readDirectory(dir),
    isDirectory: (path: string) =>
      fakeFileSystem(tree).isDirectory(path),
    readFile: (path: string) =>
      fakeFileSystem(tree).readFile(path),
  };
  const ref = indexRef(
    scan(fakeFileSystem(tree)),
  );
  return {
    app: api(ref, { fs, writer }),
    readOnlyApp: api(ref),
    ref,
    fs,
    writer,
    writes,
    tree,
  };
};

const htmlOf = (r: {
  body: ResponseBody;
}): string =>
  typeof r.body === "string" ? r.body : "";

const statusOfResponse = (r: {
  status: { content: number };
}): number => r.status.content;

const postForm = (
  path: string,
  body: string,
  query: Readonly<Record<string, string>> = {},
) => ({
  ...getRequest(path),
  method: "POST" as const,
  body,
  query,
  headers: {
    "content-type":
      "application/x-www-form-urlencoded",
  },
  bytes: none(),
});

test("GET /edit/<path> puts the document in a textarea", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        recordingWorld().app,
        getRequest("/edit/docs/a.md"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            '<textarea name="source"',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes("# before"),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<form method="post"',
          ),
        ),
      ]),
  ));

test("GET /edit/<absent> is a 404", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        recordingWorld().app,
        getRequest("/edit/docs/gone.md"),
      ),
    ),
    (r) => toBe(404)(statusOfResponse(r)),
  ));

test("a save writes the submitted source to the document's path", async () => {
  const w = recordingWorld();
  await handle(
    w.app,
    postForm(
      "/edit/docs/a.md",
      "source=%23%20after",
    ),
  );
  return check(
    w.writes,
    toEqual([["docs/a.md", "# after"]]),
  );
});

// POST/REDIRECT/GET. A rendered POST means a reload re-submits, which on a
// knowledge base is one author's edit silently overwriting a newer one.
test("a save answers 303 to the document, not a rendered page", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        recordingWorld().app,
        postForm(
          "/edit/docs/a.md",
          "source=%23%20after",
        ),
      ),
    ),
    (r) =>
      all([
        toBe(303)(statusOfResponse(r)),
        toBe("/docs/a.md")(r.headers["location"]),
      ]),
  ));

// The race this closes: the watcher debounces 50ms, and the 303 sends the
// browser straight back. Without applying the edit here, the follow-up GET
// renders the OLD source and the author watches their save not happen.
test("the index knows the new source immediately, without the watcher", async () => {
  const w = recordingWorld();
  await handle(
    w.app,
    postForm(
      "/edit/docs/a.md",
      "source=%23%20after",
    ),
  );
  return andThen(
    shouldBeOk()(
      await handle(
        w.app,
        getRequest("/api/documents/docs/a.md"),
      ),
    ),
    (r) =>
      toBe(true)(htmlOf(r).includes("# after")),
  );
});

// The save returns you to the columns you were reading, not to the root.
test("a save carries the trail back", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        recordingWorld().app,
        postForm(
          "/edit/docs/a.md",
          "source=%23%20after",
          { cols: "docs/a.md" },
        ),
      ),
    ),
    (r) =>
      toBe("/resolve/docs/a.md")(
        r.headers["location"],
      ),
  ));

// An empty body is a browser or a network losing the form, not an author
// deleting a document on purpose. Deleting is a different verb.
test("an empty save is refused and nothing is written", async () => {
  const w = recordingWorld();
  const r = await handle(
    w.app,
    postForm("/edit/docs/a.md", "source="),
  );
  return andThen(shouldBeOk()(r), (res) =>
    all([
      toBe(true)(
        htmlOf(res).includes(
          "refusing to save an empty document",
        ),
      ),
      toBe(0)(w.writes.length),
    ]),
  );
});

// Losing someone's writing to tell them it was invalid would be its own bug.
test("a refused save re-renders the form with the author's text intact", async () => {
  const w = recordingWorld();
  const r = await handle(
    w.app,
    postForm("/edit/docs/a.md", "source="),
  );
  return andThen(shouldBeOk()(r), (res) =>
    toBe(true)(htmlOf(res).includes("<textarea")),
  );
});

// CRLF cannot corrupt a render (plgg-md folds it), but it rewrites every line
// in git — a diff nobody intended is a change nobody reviewed.
test("a CRLF submission is normalised before it reaches the disk", async () => {
  const w = recordingWorld();
  await handle(
    w.app,
    postForm(
      "/edit/docs/a.md",
      "source=%23%20a%0D%0A%23%20b",
    ),
  );
  return check(
    w.writes[0]?.[1],
    toBe("# a\n# b"),
  );
});

// A front matter this repo's plgg-md cannot parse must still be SAVEABLE —
// otherwise the browser could not fix the documents /api/errors flags.
test("a save is not refused for front matter the subset declines", async () => {
  const w = recordingWorld();
  await handle(
    w.app,
    postForm(
      "/edit/docs/a.md",
      "source=---%0Afoo%3A%20%26anchor%20bar%0A---%0A%23%20body",
    ),
  );
  return check(w.writes.length, toBe(1));
});

// The capability IS the argument: an app built without a writer has no route
// that could reach a disk.
test("an app built without a writer has no edit routes at all", async () => {
  const w = recordingWorld();
  const form = await handle(
    w.readOnlyApp,
    getRequest("/edit/docs/a.md"),
  );
  const save = await handle(
    w.readOnlyApp,
    postForm("/edit/docs/a.md", "source=%23%20x"),
  );
  return andThen(shouldBeOk()(form), (f) =>
    andThen(shouldBeOk()(save), (s) =>
      all([
        // the catch-alls answer instead: /edit/... is not a document path,
        // and the POST route exists only so the response goes through the
        // no-store middleware rather than around it
        toBe(404)(statusOfResponse(f)),
        toBe(404)(statusOfResponse(s)),
        toBe("no-store, must-revalidate")(
          s.headers["cache-control"],
        ),
        toBe(0)(w.writes.length),
      ]),
    ),
  );
});

// The brand is the lock: a traversing path never reaches the writer.
test("a traversing edit path is refused before the writer sees it", async () => {
  const w = recordingWorld();
  const r = await handle(
    w.app,
    postForm(
      "/edit/../../etc/passwd.md",
      "source=%23%20x",
    ),
  );
  return andThen(shouldBeOk()(r), (res) =>
    all([
      toBe(404)(statusOfResponse(res)),
      toBe(0)(w.writes.length),
    ]),
  );
});

test("a save to a document the corpus does not hold is a 404, not a create", async () => {
  const w = recordingWorld();
  const r = await handle(
    w.app,
    postForm(
      "/edit/docs/new.md",
      "source=%23%20x",
    ),
  );
  return andThen(shouldBeOk()(r), (res) =>
    all([
      toBe(404)(statusOfResponse(res)),
      toBe(0)(w.writes.length),
    ]),
  );
});

test("the edit page carries no-store like every other response", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        recordingWorld().app,
        getRequest("/edit/docs/a.md"),
      ),
    ),
    (r) =>
      toBe("no-store, must-revalidate")(
        r.headers["cache-control"],
      ),
  ));

// ---- RBAC, through the real router ----

// Declared once and shared by the config and by every request, so a fixture
// cannot drift from the principal that is meant to hold it.
//
// They are >=16 characters because `asPrincipal` refuses anything shorter — a
// short key is not access control, it is the appearance of it. So a VALID test
// key will always look like a credential to a scanner, and the header is BUILT
// from the named fixture rather than pasted as a `Bearer <token>` literal.
// That is not hiding from the scan: a pasted token is structurally what a
// leaked credential looks like in a diff, and the scan is right to refuse to
// tell the two apart.
const READER_KEY =
  "reader-fixture-not-a-real-key";
const EDITOR_KEY =
  "editor-fixture-not-a-real-key";

const AUTHED = {
  principals: [
    {
      name: "ci-bot",
      key: READER_KEY,
      role: "reader",
    },
    {
      name: "alice",
      key: EDITOR_KEY,
      role: "editor",
    },
  ],
};

const authedWorld = () => {
  const w = recordingWorld();
  const c = asConfig(AUTHED);
  if (c.__tag === "Err") {
    throw new Error(c.content.content.message);
  }
  return {
    ...w,
    app: api(
      w.ref,
      { fs: w.fs, writer: w.writer },
      c.content,
    ),
  };
};

const withAuth = (
  req: ReturnType<typeof getRequest>,
  key: string,
) => ({
  ...req,
  headers: {
    ...req.headers,
    authorization: `Bearer ${key}`,
  },
});

test("with principals declared, an unauthenticated read is 401", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        authedWorld().app,
        getRequest("/api/health"),
      ),
    ),
    (r) =>
      all([
        toBe(401)(statusOfResponse(r)),
        // even a 401 leaves with no-store, like every other response
        toBe("no-store, must-revalidate")(
          r.headers["cache-control"],
        ),
      ]),
  ));

test("an unknown key is 401", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        authedWorld().app,
        withAuth(
          getRequest("/api/health"),
          "not-a-real-key-at-all",
        ),
      ),
    ),
    (r) => toBe(401)(statusOfResponse(r)),
  ));

test("a reader may read", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        authedWorld().app,
        withAuth(
          getRequest("/api/health"),
          READER_KEY,
        ),
      ),
    ),
    (r) => toBe(200)(statusOfResponse(r)),
  ));

// 401 vs 403 is the distinction worth keeping: 401 is "I do not know you",
// 403 is "I know, and no". Collapsing them would send a reader whose token
// works looking for a better token.
test("a reader who tries to write is 403, and nothing is written", async () => {
  const w = authedWorld();
  const r = await handle(
    w.app,
    withAuth(
      postForm(
        "/edit/docs/a.md",
        "source=%23%20x",
      ),
      READER_KEY,
    ),
  );
  return andThen(shouldBeOk()(r), (res) =>
    all([
      toBe(403)(statusOfResponse(res)),
      toBe(0)(w.writes.length),
    ]),
  );
});

test("an editor may write", async () => {
  const w = authedWorld();
  const r = await handle(
    w.app,
    withAuth(
      postForm(
        "/edit/docs/a.md",
        "source=%23%20x",
      ),
      EDITOR_KEY,
    ),
  );
  return andThen(shouldBeOk()(r), (res) =>
    all([
      toBe(303)(statusOfResponse(res)),
      toBe(1)(w.writes.length),
    ]),
  );
});

// The middleware covers every route, which is the whole reason it is a
// middleware: a per-handler check is a thing you can forget to add, and the
// one you forget is the hole.
test("every surface is behind the same gate, not just the API", async () => {
  const app = authedWorld().app;
  const paths = [
    "/",
    "/docs/a.md",
    "/api/documents",
    "/api/errors",
    "/edit/docs/a.md",
    "/anything-at-all",
  ];
  const codes = await Promise.all(
    paths.map(async (p) => {
      const r = await handle(app, getRequest(p));
      return r.__tag === "Ok"
        ? statusOfResponse(r.content)
        : 0;
    }),
  );
  return check(
    codes.every((c) => c === 401),
    toBe(true),
  );
});

// The headline case must stay frictionless: no declaration, no token.
test("with no principals declared, everything stays open", async () => {
  const w = recordingWorld();
  const read = await handle(
    w.app,
    getRequest("/api/health"),
  );
  const write = await handle(
    w.app,
    postForm("/edit/docs/a.md", "source=%23%20x"),
  );
  return andThen(shouldBeOk()(read), (a) =>
    andThen(shouldBeOk()(write), (b) =>
      all([
        toBe(200)(statusOfResponse(a)),
        toBe(303)(statusOfResponse(b)),
      ]),
    ),
  );
});
