// The SSR page, driven through plgg-server's `handle` — the same entry the
// real server runs, minus the socket.
//
// These assert on RENDERED STATE, not on eyeballed markup: the ticket's
// acceptance is "an `h3` under the second `h2` of the third `h1` renders the
// number `3-2-1.`", which is a fact about the emitted document.
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
import { api } from "#qfs-viewer/entrypoints/api";
import { RENDER_OPTIONS } from "#qfs-viewer/entrypoints/document";
import { renderMarkdownWithOptions } from "plgg-md";
import { renderToString } from "plgg-view";
import { formatOrdinal } from "#qfs-viewer/domain/model/Numbering";

const NESTED = [
  "# A",
  "# B",
  "# C",
  "## C1",
  "## C2",
  "### target",
  "",
].join("\n");

const appOver = (
  files: Readonly<Record<string, string>>,
) => api(indexRef(scan(fakeFileSystem(files))));

const htmlOf = (response: {
  body: ResponseBody;
}): string =>
  typeof response.body === "string"
    ? response.body
    : "";

const statusOfResponse = (response: {
  status: { content: number };
}): number => response.status.content;

// The ticket's worked example, end to end.
test("an h3 under the second h2 of the third h1 renders 3-2-1.", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/nest.md": NESTED }),
        getRequest("/docs/nest.md"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            '<h3 id="target">3-2-1. target</h3>',
          ),
        ),
        // its ancestors number consistently
        toBe(true)(
          htmlOf(r).includes(
            '<h1 id="c">3. C</h1>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h2 id="c2">3-2. C2</h2>',
          ),
        ),
      ]),
  ));

// Real h1-h6, never a styled div. The outline IS the structure that assistive
// tech and the MCP surface navigate by, and the ticket's verification says to
// grep for a div masquerading as a heading.
test("every heading is a real hN element carrying a stable id", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/nest.md": NESTED }),
        getRequest("/docs/nest.md"),
      ),
    ),
    (r) =>
      all([
        toBe(6)(
          (htmlOf(r).match(/<h[1-6] id="/g) ?? [])
            .length,
        ),
        toBe(false)(
          htmlOf(r).includes('<div id="target"'),
        ),
      ]),
  ));

// The number must be in the CONTENT. A CSS ::before is invisible to a screen
// reader, to curl, and to the MCP surface — all three read this same tree.
test("the number is in the heading's text, not in a style", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/nest.md": NESTED }),
        getRequest("/docs/nest.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(">3-2-1. target<"),
        ),
        toBe(false)(
          htmlOf(r).includes("::before"),
        ),
      ]),
  ));

// Measured against plgg-md 0.0.3: a skipped level is [1, 0, 1]. The zero is
// kept so it cannot collide with a real h2's [1, 1] -> "1-1.".
test("a skipped level renders a defined number with its gap visible", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({
          "docs/skip.md": "# A\n### deep\n",
        }),
        getRequest("/docs/skip.md"),
      ),
    ),
    (r) =>
      all([
        toBe(200)(statusOfResponse(r)),
        toBe(true)(
          htmlOf(r).includes(
            '<h3 id="deep">1-0-1. deep</h3>',
          ),
        ),
      ]),
  ));

test("a document opening below h1 still numbers", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/h2.md": "## B\n## C\n" }),
        getRequest("/docs/h2.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            '<h2 id="b">0-1. B</h2>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h2 id="c">0-2. C</h2>',
          ),
        ),
      ]),
  ));

// Numbers stay correct while slugs dedup — the ticket names this pair
// explicitly, because they are computed by different mechanisms over the same
// sequence.
test("repeated heading text numbers correctly while its slugs dedup", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({
          "docs/same.md":
            "# Same\n# Same\n# Same\n",
        }),
        getRequest("/docs/same.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            '<h1 id="same">1. Same</h1>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h1 id="same-1">2. Same</h1>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h1 id="same-2">3. Same</h1>',
          ),
        ),
      ]),
  ));

test("an empty document renders as a page, not a crash", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/empty.md": "" }),
        getRequest("/docs/empty.md"),
      ),
    ),
    (r) => toBe(200)(statusOfResponse(r)),
  ));

// The route resolves against the INDEX, not the filesystem: a path the corpus
// does not hold is a miss even if something exists on disk.
test("a document absent from the index is a 404 with no-store", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/nest.md": NESTED }),
        getRequest("/docs/gone.md"),
      ),
    ),
    (r) =>
      all([
        toBe(404)(statusOfResponse(r)),
        toBe("no-store, must-revalidate")(
          r.headers["cache-control"],
        ),
      ]),
  ));

// The catch-all is the same handler, so a non-markdown path answers through
// one route rather than two that could disagree.
test("a path that is not a document path is a 404, not a thrown exception", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/nest.md": NESTED }),
        getRequest("/foo"),
      ),
    ),
    (r) => toBe(404)(statusOfResponse(r)),
  ));

test("a rendered page carries no-store like every other response", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({ "docs/nest.md": NESTED }),
        getRequest("/docs/nest.md"),
      ),
    ),
    (r) =>
      toBe("no-store, must-revalidate")(
        r.headers["cache-control"],
      ),
  ));

// The URL alone says which document is on screen, so a reload lands in the
// same place and the column-accretion UI has a foundation to build on.
test("the same URL renders the same document twice — state lives in the path", async () => {
  const app = appOver({ "docs/nest.md": NESTED });
  const first = await handle(
    app,
    getRequest("/docs/nest.md"),
  );
  const second = await handle(
    app,
    getRequest("/docs/nest.md"),
  );
  return andThen(shouldBeOk()(first), (a) =>
    andThen(shouldBeOk()(second), (b) =>
      toBe(true)(
        htmlOf(a) === htmlOf(b) &&
          htmlOf(a).includes("3-2-1. target"),
      ),
    ),
  );
});

// The ticket's subtlest acceptance criterion: "The numbers in
// `MarkdownDoc.headings` and in `body` agree — they come from one counter run
// (assert directly)."
//
// It is a contract this repository DEPENDS on rather than implements: plgg-md
// builds `headings` in a separate traversal from `body`, and they agree
// because counting is a deterministic function of the heading sequence and
// both traversals walk the identical sequence. Pinned here anyway, because if
// upstream ever broke it the damage would surface far away and quietly — the
// MCP surface citing `3-1-2.` while the page shows `3-2-1.` — and this is the
// cheapest place to catch that.
test("the heading list's numbers agree with the ones rendered in the body", () => {
  const rendered = renderMarkdownWithOptions(
    RENDER_OPTIONS,
  )(NESTED);
  if (rendered.__tag === "Err") {
    return toBe("rendered")("Err");
  }
  const html = renderToString(
    rendered.content.body,
  );
  return all(
    rendered.content.headings.map((h) =>
      // every number the heading list reports is the number the body carries,
      // on the element with that heading's slug
      toBe(true)(
        html.includes(
          `<h${h.level} id="${h.slug}">${formatOrdinal(h.ordinal)} `,
        ),
      ),
    ),
  );
});

// The heading ladder has six arms and the tests above only ever reached three,
// so h4/h5/h6 were emitted by code no test had run. A six-deep document walks
// the whole ladder.
test("every level from h1 to h6 renders as its own element, numbered", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({
          "docs/deep.md":
            "# a\n## b\n### c\n#### d\n##### e\n###### f\n",
        }),
        getRequest("/docs/deep.md"),
      ),
    ),
    (r) =>
      all([
        toBe(true)(
          htmlOf(r).includes(
            '<h1 id="a">1. a</h1>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h4 id="d">1-1-1-1. d</h4>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h5 id="e">1-1-1-1-1. e</h5>',
          ),
        ),
        toBe(true)(
          htmlOf(r).includes(
            '<h6 id="f">1-1-1-1-1-1. f</h6>',
          ),
        ),
      ]),
  ));

// A document the corpus HAS but cannot render is a 500, not a 404: telling a
// reader it does not exist would send them looking for a missing file instead
// of a broken one. The index keeps such a document (skip-and-collect), so this
// path is reachable.
test("a document that will not render is a 500, not a 404", async () =>
  andThen(
    shouldBeOk()(
      await handle(
        appOver({
          "docs/bad.md":
            "---\ntitle: x\nno closing fence",
        }),
        getRequest("/docs/bad.md"),
      ),
    ),
    (r) => toBe(500)(statusOfResponse(r)),
  ));
