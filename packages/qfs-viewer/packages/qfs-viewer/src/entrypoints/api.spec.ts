// The REST API, driven through plgg-server's plgg-native `handle`.
//
// No port, no sockets, no sleeps: `handle(app, request)` is the same entry the
// real server runs, minus the platform seam, so these assertions are about
// behaviour rather than about a listening socket
// (workaholic:implementation / test — judge by rendered state).
import {
  test,
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
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import { indexRef } from "#qfs-viewer/domain/usecase/reload";
import { api } from "#qfs-viewer/entrypoints/api";

const CORPUS = {
  "docs/a.md": "# alpha",
  "docs/deep/b.md": "# beta",
  ".workaholic/terms/index.md": "# terms",
};

// `getRequest` is plgg-server's SSG-crawler GET: it hardcodes `query: {}` and
// does not parse a query string off the path, so `"/api/documents?x=1"` would
// simply 404 as an unknown route. The query bag is what `HttpRequest` actually
// carries, so tests fill it directly — this is the same value the platform
// seam builds from a real URL.
const getWithQuery = (
  path: string,
  query: Readonly<Record<string, string>>,
) => ({ ...getRequest(path), query });

// `status` is a `Box<"HttpStatus", number>`, not a number.
const statusOfResponse = (response: {
  status: { content: number };
}): number => response.status.content;

const appOver = (
  files: Readonly<
    Record<string, string>
  > = CORPUS,
  unreadable: ReadonlySet<string> = new Set(),
) =>
  api(
    indexRef(
      scan(fakeFileSystem(files, unreadable)),
    ),
  );

// A response body is `SoftStr | Box<"Bytes"> | Box<"Stream">`. The API only
// ever emits JSON text, so anything else here is a real failure — narrow with
// `typeof` (never a cast) and fail loudly rather than quietly parsing "".
const bodyOf = (response: {
  body: ResponseBody;
}): unknown =>
  typeof response.body === "string"
    ? JSON.parse(response.body)
    : { unexpectedNonTextBody: true };

test("GET /api/documents lists every document, path-ordered", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest("/api/documents"),
      ),
    ),
    (r) =>
      toEqual({
        count: 3,
        totalCount: 3,
        limit: 20,
        offset: 0,
        documents: [
          { path: ".workaholic/terms/index.md" },
          { path: "docs/a.md" },
          { path: "docs/deep/b.md" },
        ],
      })(bodyOf(r)),
  ));

test("GET /api/documents/<known> returns the document's exact source", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest(
          "/api/documents/docs/deep/b.md",
        ),
      ),
    ),
    (r) =>
      toEqual({
        path: "docs/deep/b.md",
        frontMatter: null,
        source: "# beta",
      })(bodyOf(r)),
  ));

test("GET /api/documents/<absent> is a 404, never a throw", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest("/api/documents/docs/nope.md"),
      ),
    ),
    (r) => toBe(404)(r.status.content),
  ));

test("a 404 carries no-store too — a cached miss hides a new document", async () =>
  // The bug a live curl found that the unit tests missed: the header was set
  // on the success path only, so 404s came back with no cache-control at all.
  // Cache a "not found" and the document someone just added stays missing.
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest("/api/documents/docs/nope.md"),
      ),
    ),
    (r) =>
      all([
        toBe(404)(r.status.content),
        toBe("no-store, must-revalidate")(
          r.headers["cache-control"],
        ),
      ]),
  ));

test("an unmatched path is a 404 that still carries no-store", async () =>
  // Found by a live curl, not by a unit test: plgg-server answers an
  // unmatched path with its own 404, which never passes through global `use()`
  // middleware — so /foo came back with no cache-control at all. The catch-all
  // route exists to close that hole; this pins it.
  all([
    andThen(
      shouldBeOk()(
        await handle(
          appOver(),
          getRequest("/foo"),
        ),
      ),
      (r) =>
        all([
          toBe(404)(r.status.content),
          toBe("no-store, must-revalidate")(
            r.headers["cache-control"],
          ),
        ]),
    ),
    andThen(
      shouldBeOk()(
        await handle(
          appOver(),
          getRequest("/api/nope"),
        ),
      ),
      (r) =>
        all([
          toBe(404)(r.status.content),
          toBe("no-store, must-revalidate")(
            r.headers["cache-control"],
          ),
        ]),
    ),
  ]));

test("a traversing path is refused and reads nothing outside the corpus", async () => {
  // The real security surface. The fake filesystem below WOULD happily serve
  // `secret.md` if the handler asked for it — so a pass here means the guard
  // ran, not that the file merely happened to be missing.
  const app = appOver({
    "docs/a.md": "# alpha",
    "secret.md": "# do not serve",
  });
  return all([
    andThen(
      shouldBeOk()(
        await handle(
          app,
          getRequest(
            "/api/documents/docs/../secret.md",
          ),
        ),
      ),
      (r) => toBe(404)(r.status.content),
    ),
    andThen(
      shouldBeOk()(
        await handle(
          app,
          getRequest(
            "/api/documents/../../etc/passwd.md",
          ),
        ),
      ),
      (r) => toBe(404)(r.status.content),
    ),
  ]);
});

test("every response carries no-store — a stale document is an incident", async () =>
  // docs/adr/0003-no-caching.md, asserted rather than remembered.
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest("/api/health"),
      ),
    ),
    (r) =>
      toBe("no-store, must-revalidate")(
        r.headers["cache-control"],
      ),
  ));

test("GET /api/errors surfaces a corpus's failures rather than hiding them", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(
          {
            "docs/a.md": "# alpha",
            "docs/vanished.md": "# gone",
          },
          new Set(["docs/vanished.md"]),
        ),
        getRequest("/api/errors"),
      ),
    ),
    (r) => {
      const body = bodyOf(r);
      const errors =
        typeof body === "object" &&
        body !== null &&
        "errors" in body &&
        Array.isArray(body.errors)
          ? body.errors
          : [];
      return all([
        toBe(1)(errors.length),
        toBe("docs/vanished.md")(errors[0]?.path),
      ]);
    },
  ));

test("GET /api/health reports the corpus at a glance", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest("/api/health"),
      ),
    ),
    (r) =>
      toEqual({
        documentCount: 3,
        errorCount: 0,
      })(bodyOf(r)),
  ));

test("the wildcard route does not shadow /api/documents", async () =>
  // Registration order matters: a document path contains slashes, so a
  // greedy wildcard could swallow the list endpoint.
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getRequest("/api/documents"),
      ),
    ),
    (r) => {
      const body = bodyOf(r);
      return toBe(true)(
        typeof body === "object" &&
          body !== null &&
          "count" in body,
      );
    },
  ));

test("a request reads ONE index: a swap mid-flight cannot tear the response", async () => {
  // The claim IndexRef's doc comment makes, exercised by its first real
  // consumer rather than asserted.
  const ref = indexRef(
    scan(fakeFileSystem({ "docs/a.md": "# v1" })),
  );
  const app = api(ref);
  const inflight = handle(
    app,
    getRequest("/api/documents/docs/a.md"),
  );
  // Swap while the request is in flight.
  ref.swap(
    scan(fakeFileSystem({ "docs/a.md": "# v2" })),
  );
  const settled = await inflight;
  return all([
    // the in-flight read saw a consistent index (the one it started with)
    andThen(shouldBeOk()(settled), (r) =>
      toEqual({
        path: "docs/a.md",
        frontMatter: null,
        source: "# v1",
      })(bodyOf(r)),
    ),
    // and a request starting now sees the new corpus
    andThen(
      shouldBeOk()(
        await handle(
          app,
          getRequest("/api/documents/docs/a.md"),
        ),
      ),
      (r) =>
        toEqual({
          path: "docs/a.md",
          frontMatter: null,
          source: "# v2",
        })(bodyOf(r)),
    ),
  ]);
});

// The tag-group read, end to end through the real router: front matter parsed
// by plgg-md, filtered by the domain, served as JSON. This is the mission's
// headline query, so it is asserted at the surface a consumer actually calls
// rather than only at the domain function beneath it.
const FACETED = {
  "docs/a.md":
    "---\ntype: bugfix\nlayer:\n  - Domain\n---\nalpha",
  "docs/b.md":
    "---\ntype: enhancement\n---\nbravo",
  "docs/c.md": "# no front matter\ncharlie",
};

test("GET /api/documents?type=bugfix facets on front matter", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(FACETED),
        getWithQuery("/api/documents", {
          type: "bugfix",
        }),
      ),
    ),
    (r) =>
      toEqual({
        count: 3,
        totalCount: 1,
        limit: 20,
        offset: 0,
        documents: [{ path: "docs/a.md" }],
      })(bodyOf(r)),
  ));

test("GET /api/documents?limit=1 pages, and totalCount ignores the window", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(FACETED),
        getWithQuery("/api/documents", {
          limit: "1",
          offset: "1",
        }),
      ),
    ),
    (r) =>
      toEqual({
        count: 3,
        totalCount: 3,
        limit: 1,
        offset: 1,
        documents: [{ path: "docs/b.md" }],
      })(bodyOf(r)),
  ));

// A broken caller learns of its bug. The alternative — quietly serving 20
// documents — is the behaviour `plgg-cms/model/ListQuery.ts` refuses, and the
// reason the parse rejects rather than clamps.
test("GET /api/documents?limit=abc is a 400 that names the field", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(FACETED),
        getWithQuery("/api/documents", {
          limit: "abc",
        }),
      ),
    ),
    (r) =>
      all([
        toBe(400)(statusOfResponse(r)),
        toBe(true)(
          typeof r.body === "string" &&
            r.body.includes("limit"),
        ),
        // The no-store middleware covers the error path too — the bug a live
        // curl of a 404 caught once already.
        toBe("no-store, must-revalidate")(
          r.headers["cache-control"],
        ),
      ]),
  ));

test("GET /api/documents/<path> serves the parsed front matter", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(FACETED),
        getRequest("/api/documents/docs/a.md"),
      ),
    ),
    (r) =>
      toEqual({
        path: "docs/a.md",
        frontMatter: {
          type: "bugfix",
          layer: ["Domain"],
        },
        source:
          "---\ntype: bugfix\nlayer:\n  - Domain\n---\nalpha",
      })(bodyOf(r)),
  ));

// ---- GET /qfs: the form's one translation, path -> trail segment ----

// A form cannot write itself into `cols`, so this route does it: validate
// the typed path, append it to the carried trail, answer 303 to the trail
// URL. The screen the reader lands on IS an address.
test("GET /qfs appends the typed path to the trail and redirects", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getWithQuery("/qfs", {
          path: "/local/repo/docs",
          cols: "docs/a.md",
        }),
      ),
    ),
    (r) =>
      all([
        toBe(303)(statusOfResponse(r)),
        toBe(
          "/resolve/docs/a.md,qfs:/local/repo/docs",
        )(r.headers["location"] ?? ""),
      ]),
  ));

test("GET /qfs with no trail starts one", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getWithQuery("/qfs", {
          path: "/local/repo",
        }),
      ),
    ),
    (r) =>
      toBe("/resolve/qfs:/local/repo")(
        r.headers["location"] ?? "",
      ),
  ));

// The path is untrusted input: whitespace, quotes and pipes never reach a
// statement, and the refusal names the rule for the person who typed it.
test("GET /qfs refuses what is not a qfs path, naming the rule", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getWithQuery("/qfs", {
          path: "/a |> remove /b",
        }),
      ),
    ),
    (r) =>
      all([
        toBe(400)(statusOfResponse(r)),
        toBe(true)(
          typeof r.body === "string" &&
            r.body.includes("not a qfs path"),
        ),
        // the error path leaves through noStore like every other response
        toBe("no-store, must-revalidate")(
          r.headers["cache-control"],
        ),
      ]),
  ));

test("GET /qfs with no path at all is a 400, not a crash", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(),
        getWithQuery("/qfs", {}),
      ),
    ),
    (r) => toBe(400)(statusOfResponse(r)),
  ));
