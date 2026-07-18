// The MCP tools, called directly.
//
// `runStdioServer` is the IO seam and is not exercised here — these call the
// registry the way the dispatcher does, which is where the behaviour lives.
// The wire itself is driven live (real JSON-RPC frames into the real bin).
import {
  test,
  check,
  all,
  toBe,
  toEqual,
  shouldBeOk,
  andThen,
} from "plgg-test";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import { indexRef } from "#qfs-viewer/domain/usecase/reload";
import { insightTools } from "#qfs-viewer/entrypoints/mcpTools";
import { type ToolResult } from "plgg-mcp";

const CORPUS = {
  "docs/a.md": [
    "---",
    "type: adr",
    "layer: [Domain, Infrastructure]",
    "---",
    "# alpha",
    "",
  ].join("\n"),
  "docs/b.md": [
    "---",
    "type: adr",
    "---",
    "# bravo",
    "",
  ].join("\n"),
  "docs/c.md": "# charlie, no front matter\n",
};

const tools = (
  files: Readonly<
    Record<string, string>
  > = CORPUS,
) =>
  insightTools(
    indexRef(scan(fakeFileSystem(files))),
  );

const toolNamed = (
  name: string,
  files?: Readonly<Record<string, string>>,
) => {
  const t = tools(files).find(
    (x) => x.name === name,
  );
  if (t === undefined) {
    throw new Error(`no such tool: ${name}`);
  }
  return t;
};

const textOf = (r: ToolResult): string =>
  r.content[0]?.text ?? "";

const dataOf = (r: ToolResult): unknown =>
  JSON.parse(textOf(r));

test("the registry exposes the four read tools", () =>
  check(
    tools().map((t) => t.name),
    toEqual([
      "list_documents",
      "get_document",
      "list_tag_groups",
      "corpus_health",
    ]),
  ));

// Every tool must describe itself well enough for an agent to choose it
// without reading this source — the description IS the interface.
test("every tool carries a description and an input schema", () =>
  all(
    tools().flatMap((t) => [
      check(
        t.description.length > 40,
        toBe(true),
      ),
      check(
        typeof t.inputSchema === "object" &&
          t.inputSchema !== null,
        toBe(true),
      ),
    ]),
  ));

test("list_documents lists the corpus", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("list_documents").call({}),
    ),
    (r) =>
      all([
        toBe(false)(r.isError),
        toEqual({
          totalCount: 3,
          corpusCount: 3,
          limit: 20,
          offset: 0,
          documents: [
            "docs/a.md",
            "docs/b.md",
            "docs/c.md",
          ],
        })(dataOf(r)),
      ]),
  ));

// The mission's claim, asserted: an agent's facet is the SAME facet the HTTP
// surface answers, because both call `listCollection` over one index.
test("an unreserved argument facets on front matter, as it does over HTTP", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("list_documents").call({
        type: "adr",
      }),
    ),
    (r) =>
      toEqual({
        totalCount: 2,
        corpusCount: 3,
        limit: 20,
        offset: 0,
        documents: ["docs/a.md", "docs/b.md"],
      })(dataOf(r)),
  ));

test("a sequence answers to any of its members", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("list_documents").call({
        layer: "Infrastructure",
      }),
    ),
    (r) =>
      check(
        dataOf(r),
        toEqual({
          totalCount: 1,
          corpusCount: 3,
          limit: 20,
          offset: 0,
          documents: ["docs/a.md"],
        }),
      ),
  ));

// A malformed argument is a TOOL error, not a protocol error: the agent asked
// a bad question and should learn which field, so it can ask a better one
// rather than retry the same call.
test("a bad argument is an isError result naming the field, not a throw", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("list_documents").call({
        limit: "abc",
      }),
    ),
    (r) =>
      all([
        toBe(true)(r.isError),
        toBe(true)(textOf(r).includes("limit")),
      ]),
  ));

test("get_document returns the source and the parsed front matter", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("get_document").call({
        path: "docs/a.md",
      }),
    ),
    (r) =>
      all([
        toBe(false)(r.isError),
        toBe(true)(textOf(r).includes("# alpha")),
        toBe(true)(textOf(r).includes('"adr"')),
      ]),
  ));

test("get_document on a fence-less document reports null front matter", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("get_document").call({
        path: "docs/c.md",
      }),
    ),
    (r) =>
      toBe(true)(
        textOf(r).includes('"frontMatter": null'),
      ),
  ));

// The arguments arrive as `unknown` off the wire; the tool decodes them
// itself, fail-closed. The dispatcher is not trusted and neither is the agent.
test("get_document fails closed on a missing or non-string path", async () => {
  const t = toolNamed("get_document");
  const noPath = await t.call({});
  const numberPath = await t.call({ path: 42 });
  const notAnObject = await t.call("docs/a.md");
  return andThen(shouldBeOk()(noPath), (a) =>
    andThen(shouldBeOk()(numberPath), (b) =>
      andThen(shouldBeOk()(notAnObject), (c) =>
        all([
          toBe(true)(a.isError),
          toBe(true)(b.isError),
          toBe(true)(c.isError),
        ]),
      ),
    ),
  );
});

test("get_document rejects a traversing path at the boundary", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("get_document").call({
        path: "../../etc/passwd.md",
      }),
    ),
    (r) =>
      all([
        toBe(true)(r.isError),
        toBe(true)(
          textOf(r).includes(
            "not a document path",
          ),
        ),
      ]),
  ));

// A miss points the agent at the tool that would have answered — a dead end
// that names its own way out.
test("get_document on an absent document says so and names list_documents", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("get_document").call({
        path: "docs/gone.md",
      }),
    ),
    (r) =>
      all([
        toBe(true)(r.isError),
        toBe(true)(
          textOf(r).includes("list_documents"),
        ),
      ]),
  ));

test("list_tag_groups reports the corpus's own dimensions with counts", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("list_tag_groups").call({}),
    ),
    (r) =>
      // `layer` first: it reaches 2 documents (Domain + Infrastructure) and so
      // does `type`, and equal reach is broken by key so the order cannot
      // shuffle between reloads. My first expectation had them the other way
      // round — the implementation was right.
      check(
        dataOf(r),
        toEqual({
          groups: [
            {
              key: "layer",
              variations: [
                { value: "Domain", count: 1 },
                {
                  value: "Infrastructure",
                  count: 1,
                },
              ],
            },
            {
              key: "type",
              variations: [
                { value: "adr", count: 2 },
              ],
            },
          ],
        }),
      ),
  ));

test("corpus_health counts documents and names what could not be read", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("corpus_health", {
        "docs/a.md": "# fine",
        "docs/bad.md":
          "---\nfoo: &anchor x\n---\n# body",
      }).call({}),
    ),
    (r) =>
      all([
        toBe(false)(r.isError),
        toBe(true)(
          textOf(r).includes(
            '"documentCount": 2',
          ),
        ),
        toBe(true)(
          textOf(r).includes('"errorCount": 1'),
        ),
        toBe(true)(
          textOf(r).includes("docs/bad.md"),
        ),
      ]),
  ));

test("an empty corpus answers rather than failing", async () =>
  andThen(
    shouldBeOk()(
      await toolNamed("list_documents", {}).call(
        {},
      ),
    ),
    (r) =>
      all([
        toBe(false)(r.isError),
        toBe(true)(
          textOf(r).includes('"totalCount": 0'),
        ),
      ]),
  ));
