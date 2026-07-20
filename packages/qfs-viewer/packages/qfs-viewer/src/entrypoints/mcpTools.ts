// The MCP tools: the corpus, reachable by an agent.
//
// The mission's third surface, and the point is that it is a SURFACE and not a
// second product. Every tool below reads the same `Index` through the same
// `listCollection` / `tagGroupsOf` / `getDocument` the REST API and the column
// UI read. "What a developer reads on screen, a script fetches, and an agent
// queries over MCP are the same documents with the same tags" is only true if
// nothing here re-implements the query — so nothing here does.
//
// The protocol machinery is `plgg-mcp`'s: a hand-rolled JSON-RPC 2.0 / MCP
// built on plgg primitives and the Node stdlib, with no
// `@modelcontextprotocol/sdk`. That matters for ADR 0001 — an MCP SDK would be
// a non-plgg runtime dependency, and ADR 0006 already rejected hand-rolling a
// protocol (OTLP) as disproportionate. plgg-mcp resolves that tension by being
// plgg-family: the protocol is upstream's, the tools are ours.
//
// READ-ONLY, and the reason has changed now that principals exist — so it is
// worth restating rather than leaving the old one to rot.
//
// This transport is STDIO. There is no network boundary and therefore no
// credential to check: the principal is whoever started the process, and the
// OS authenticated them when they ran it. `qfs-viewer.config.json`'s
// principals govern the HTTP surface, where a request arrives from somewhere
// else; they have nothing to say here, and pretending to check a token over a
// pipe would be theatre.
//
// So the case for a write tool is no longer blocked on authorization — it is
// simply not built. When it is, it inherits the OS's answer about who is
// asking, which is the honest one for a pipe. A NETWORK MCP transport would be
// a different question entirely, and would go through `authorize` like every
// other route.
import {
  ok,
  type PromisedResult,
  type Defect,
} from "plgg";
import {
  type Tool,
  type ToolRegistry,
  type ToolResult,
} from "plgg-mcp";
import { matchOption } from "plgg";
import { type FrontMatter } from "#qfs-viewer/domain/model/Document";
import {
  type Index,
  getDocument,
  documentCount,
  indexErrors,
  // Aliased: `listDocuments` is already this file's tool factory.
  listDocuments as allDocuments,
} from "#qfs-viewer/domain/model/Index";
import { listCollection } from "#qfs-viewer/domain/usecase/listCollection";
import { tagGroupsOf } from "#qfs-viewer/domain/usecase/tagGroups";
import { parseListQuery } from "#qfs-viewer/domain/model/Query";
import { type IndexRef } from "#qfs-viewer/domain/usecase/reload";
import {
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

// A tool never throws and never returns a bare string: MCP wants content
// blocks, and a client renders `isError` differently from a result.
const textResult = (
  text: string,
  isError = false,
): ToolResult => ({
  content: [{ type: "text", text }],
  isError,
});

const jsonResult = (data: unknown): ToolResult =>
  textResult(JSON.stringify(data, null, 2));

const failure = (message: string): ToolResult =>
  textResult(message, true);

// The arguments arrive as `unknown` from the wire. Every tool decodes them
// itself, fail-closed — the dispatcher never trusts the shape, and neither
// does this (workaholic:implementation / type-driven-design).
const stringField = (
  args: unknown,
  key: string,
): string | undefined => {
  if (
    typeof args !== "object" ||
    args === null ||
    !(key in args)
  ) {
    return undefined;
  }
  const value: unknown = Reflect.get(args, key);
  return typeof value === "string"
    ? value
    : undefined;
};

// Everything that is not a reserved list parameter is a front-matter filter,
// exactly as it is over HTTP — one query vocabulary, three surfaces.
const queryParamsOf = (
  args: unknown,
): Readonly<Record<string, string>> => {
  if (typeof args !== "object" || args === null) {
    return {};
  }
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(
    args,
  )) {
    if (typeof value === "string") {
      out[key] = value;
    } else if (
      typeof value === "number" ||
      typeof value === "boolean"
    ) {
      out[key] = String(value);
    }
  }
  return out;
};

const listDocuments = (ref: IndexRef): Tool => ({
  name: "list_documents",
  description:
    'List the repository\'s markdown documents, optionally faceted by front-matter tag. Any argument other than limit/offset/order/q is a front-matter filter: {"type": "bugfix"} lists the bugfix tickets, and a document whose front matter says layer: [Domain, Infrastructure] answers to {"layer": "Domain"}. Returns paths and totalCount; fetch a document\'s text with get_document.',
  inputSchema: {
    type: "object",
    properties: {
      q: {
        type: "string",
        description:
          "free text matched against the document's source",
      },
      limit: {
        type: "string",
        description:
          "page size, 1-100 (default 20)",
      },
      offset: {
        type: "string",
        description:
          "how many to skip (default 0)",
      },
      order: {
        type: "string",
        enum: ["asc", "desc"],
        description: "path order (default asc)",
      },
    },
    additionalProperties: { type: "string" },
  },
  call: (
    args: unknown,
  ): PromisedResult<ToolResult, Defect> => {
    // Read the index ONCE, like every other surface: a reload mid-call cannot
    // tear the answer.
    const index: Index = ref.current();
    const query = parseListQuery(
      queryParamsOf(args),
    );
    if (query.__tag === "Err") {
      // A bad argument is a TOOL error, not a protocol error: the agent asked
      // a malformed question and should be told which field, so it can ask a
      // better one rather than retry the same thing.
      return Promise.resolve(
        ok(
          failure(query.content.content.message),
        ),
      );
    }
    const page = listCollection(
      index,
      query.content,
    );
    return Promise.resolve(
      ok(
        jsonResult({
          totalCount: page.totalCount,
          corpusCount: documentCount(index),
          limit: page.limit,
          offset: page.offset,
          documents: page.contents.map((d) =>
            documentPathString(d.content.path),
          ),
        }),
      ),
    );
  },
});

const getDocumentTool = (
  ref: IndexRef,
): Tool => ({
  name: "get_document",
  description:
    "Fetch one document by its repository-relative path (as returned by list_documents), with its parsed front matter and full markdown source.",
  inputSchema: {
    type: "object",
    properties: {
      path: {
        type: "string",
        description:
          "repository-relative path, e.g. docs/adr/0001-npm-only.md",
      },
    },
    required: ["path"],
  },
  call: (
    args: unknown,
  ): PromisedResult<ToolResult, Defect> => {
    const index: Index = ref.current();
    const raw = stringField(args, "path");
    if (raw === undefined) {
      return Promise.resolve(
        ok(
          failure(
            "path is required and must be a string",
          ),
        ),
      );
    }
    const path = asDocumentPath(raw);
    if (path.__tag === "Err") {
      return Promise.resolve(
        ok(
          failure(
            `not a document path: ${JSON.stringify(raw)} (want a relative, non-traversing .md path)`,
          ),
        ),
      );
    }
    const doc = getDocument(index, path.content);
    return Promise.resolve(
      doc.__tag === "None"
        ? ok(
            failure(
              `no such document: ${raw}. Use list_documents to see what the corpus holds.`,
            ),
          )
        : ok(
            jsonResult({
              path: documentPathString(
                doc.content.content.path,
              ),
              frontMatter: matchOption<
                FrontMatter,
                FrontMatter | null
              >(
                () => null,
                (fields) => fields,
              )(doc.content.content.frontMatter),
              source: doc.content.content.source,
            }),
          ),
    );
  },
});

const listTagGroups = (ref: IndexRef): Tool => ({
  name: "list_tag_groups",
  description:
    "List the dimensions this corpus can be navigated by, with each dimension's variations and how many documents carry them. These are discovered from the documents' own front matter, so they describe THIS repository rather than a fixed schema. Feed a key/value pair back to list_documents to facet on it.",
  inputSchema: { type: "object", properties: {} },
  call: (): PromisedResult<
    ToolResult,
    Defect
  > => {
    const index: Index = ref.current();
    return Promise.resolve(
      ok(
        jsonResult({
          // The whole corpus, because this tool takes no filter: the set it
          // counts over and the set it describes are the same one.
          groups: tagGroupsOf(
            allDocuments(index),
          ).map((g) => ({
            key: g.key,
            variations: g.values.map((v) => ({
              value: v,
              count: g.counts[v] ?? 0,
            })),
          })),
        }),
      ),
    );
  },
});

const corpusHealth = (ref: IndexRef): Tool => ({
  name: "corpus_health",
  description:
    "The corpus at a glance: how many documents are indexed and which ones could not be read. A non-zero error count is not necessarily a fault — a document whose front matter plgg-md's YAML subset declines is still indexed and served, it simply cannot be faceted.",
  inputSchema: { type: "object", properties: {} },
  call: (): PromisedResult<
    ToolResult,
    Defect
  > => {
    const index: Index = ref.current();
    return Promise.resolve(
      ok(
        jsonResult({
          documentCount: documentCount(index),
          errorCount: indexErrors(index).length,
          errors: indexErrors(index).map((e) => ({
            path: e.content.path,
            message: e.content.message,
          })),
        }),
      ),
    );
  },
});

/**
 * Every tool this server exposes, over a live index.
 *
 * Takes the `IndexRef` rather than an `Index` for the same reason the HTTP API
 * does: the tools must see the CURRENT corpus after a hot reload, while each
 * individual call still reads one consistent value.
 */
export const insightTools = (
  ref: IndexRef,
): ToolRegistry => [
  listDocuments(ref),
  getDocumentTool(ref),
  listTagGroups(ref),
  corpusHealth(ref),
];
