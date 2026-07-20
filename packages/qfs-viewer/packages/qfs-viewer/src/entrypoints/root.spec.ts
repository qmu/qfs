// What `GET /` owes a person, whichever view happens to serve it.
//
// These predate the column UI: they were written against the first root page,
// the one that existed because the honest answer to "what is at the root" had
// been `404 Not Found`. `columns.ts` serves `/` now, and every one of these
// still holds — which is the point of keeping them in a file named for the
// ROUTE rather than for the module. They are the properties that must survive
// the next rewrite of the view too.
//
// One of them earned its keep during that rewrite: the columns view dropped
// the scan-error section, and "reports scan errors rather than hiding them"
// caught it. A document that silently never indexed is exactly the bug this
// makes visible.
import {
  test,
  all,
  toBe,
  toContain,
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

const appOver = (
  files: Readonly<Record<string, string>>,
  unreadable: ReadonlySet<string> = new Set(),
) =>
  api(
    indexRef(
      scan(fakeFileSystem(files, unreadable)),
    ),
  );

const htmlOf = (response: {
  body: ResponseBody;
}): string =>
  typeof response.body === "string"
    ? response.body
    : "<non-text body>";

const CORPUS = {
  "docs/a.md": "# alpha",
  ".workaholic/terms/index.md": "# terms",
};

test("GET / is a page, not a 404 — the root a person types must explain itself", async () =>
  // The whole reason this handler exists: `/` answered `404 Not Found` while
  // the server was working perfectly, which reads as a broken deployment.
  andThen(
    shouldBeOk()(
      await handle(
        appOver(CORPUS),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(r.status.content),
        toContain("text/html")(
          r.headers["content-type"] ?? "",
        ),
        toContain("qfs-viewer")(htmlOf(r)),
      ]),
  ));

test("GET / lists the real corpus", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(CORPUS),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toContain("docs/a.md")(htmlOf(r)),
        toContain(".workaholic/terms/index.md")(
          htmlOf(r),
        ),
      ]),
  ));

test("GET / renders semantic headings, not styled divs", async () =>
  // accessibility-first: the structure is what assistive tech and the MCP
  // surface both read.
  andThen(
    shouldBeOk()(
      await handle(
        appOver(CORPUS),
        getRequest("/"),
      ),
    ),
    (r) =>
      all([
        toContain("<h1")(htmlOf(r)),
        toContain("<h2")(htmlOf(r)),
        toContain("<ul")(htmlOf(r)),
        toContain("<li")(htmlOf(r)),
      ]),
  ));

test("GET / carries no-store like every other response", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(CORPUS),
        getRequest("/"),
      ),
    ),
    (r) =>
      toBe("no-store, must-revalidate")(
        r.headers["cache-control"],
      ),
  ));

test("GET / reports scan errors rather than hiding them", async () =>
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
        getRequest("/"),
      ),
    ),
    (r) =>
      toContain("docs/vanished.md")(htmlOf(r)),
  ));

test("GET / on an empty corpus is still a page, not a crash", async () =>
  andThen(
    shouldBeOk()(
      await handle(appOver({}), getRequest("/")),
    ),
    (r) =>
      all([
        toBe(200)(r.status.content),
        toContain("qfs-viewer")(htmlOf(r)),
      ]),
  ));

test("the root does not shadow the API", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver(CORPUS),
        getRequest("/api/health"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(r.status.content),
        toContain("documentCount")(htmlOf(r)),
      ]),
  ));
