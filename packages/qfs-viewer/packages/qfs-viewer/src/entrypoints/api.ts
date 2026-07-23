// The REST API: the corpus as JSON.
//
// A thin program checkpoint (workaholic:implementation /
// anti-corruption-structure). It reads argv-free: parse the request, call the
// domain's public procedures, format the result. No domain logic lives here —
// which is why this file may name plgg-server while `domain/` may not, and
// why the same `scan`/`Index` procedures back the CLI, this API, and the SSR
// and MCP surfaces to come. That all four can start the same procedure
// identically is the evidence the separation held.
//
// Written against plgg-http's `HttpRequest`/`HttpResponse`, never a platform
// `Request`/`Response`: `toFetch` is the only seam that touches those
// (workaholic:design / vendor-neutrality), which is what keeps a future
// Worker/Lambda adapter an IO conversion rather than a rewrite.
import {
  pipe,
  ok,
  err,
  isOk,
  matchOption,
  fromNullable,
  type PromisedResult,
} from "plgg";
import { type FrontMatter } from "#qfs-viewer/domain/model/Document";
import {
  web,
  get,
  post,
  use,
  jsonResponse,
  textResponse,
  notFound,
  badRequest,
  unauthorized,
  forbidden,
  statusOf,
  httpErrorToResponse,
  type Web,
  type Context,
  type Middleware,
  type HttpResponse,
  type HttpError,
} from "plgg-server";
import { parseListQuery } from "#qfs-viewer/domain/model/Query";
import { listCollection } from "#qfs-viewer/domain/usecase/listCollection";
import {
  type Index,
  documentCount,
  indexErrors,
  getDocument,
} from "#qfs-viewer/domain/model/Index";
import { type IndexRef } from "#qfs-viewer/domain/usecase/reload";
import {
  columnsHandler,
  resolveHandler,
} from "#qfs-viewer/entrypoints/columns";
import {
  editFormHandler,
  editSaveHandler,
} from "#qfs-viewer/entrypoints/edit";
import { type FileWriter } from "#qfs-viewer/domain/model/Editor";
import {
  type FileSystem,
  type ResourceRunner,
} from "#qfs-viewer/domain/model/Scan";
import {
  type Config,
  defaultConfig,
} from "#qfs-viewer/domain/model/Config";
import {
  accessFor,
  bearerOf,
  mayRead,
  mayEdit,
} from "#qfs-viewer/domain/model/Principal";
import { documentPageHandler } from "#qfs-viewer/entrypoints/document";
import {
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";
import { asQfsPath } from "#qfs-viewer/domain/model/Describe";
import {
  parseTrail,
  trailUrl,
  openFrom,
  qfsStop,
} from "#qfs-viewer/domain/model/Trail";

const NO_STORE = "no-store, must-revalidate";

/**
 * Stamp `cache-control: no-store` on EVERY response — including the ones a
 * handler never built.
 *
 * A middleware rather than a per-handler header, because a 404 is exactly the
 * response that most needs it: cache a "document not found" and the document
 * someone just added stays missing. The first cut set the header only on the
 * success path, and a live `curl` of a 404 came back with no `cache-control`
 * at all — the unit test had asserted the header on a 200 and proved nothing
 * about the error path.
 *
 * So the typed `HttpError` is folded into its response HERE, at the edge (the
 * `mapErr(toHttpError)`-once-at-the-edge idiom), and the header goes on
 * whatever comes out. The route handlers keep returning `err(notFound(...))` —
 * they stay honest about failure; only the transport flattens it, which is
 * what HTTP does anyway: a 404 is a response, not a program error
 * (docs/adr/0003-no-caching.md).
 */
const noStore: Middleware = async (c, next) => {
  const outcome = await next(c);
  const response = isOk(outcome)
    ? outcome.content
    : httpErrorToResponse(outcome.content);
  return ok({
    ...response,
    headers: {
      ...response.headers,
      "cache-control": NO_STORE,
    },
  });
};

const json = (data: unknown): HttpResponse =>
  jsonResponse(data, statusOf(200));

/**
 * Refuse anyone this corpus did not declare — and refuse a reader who tries to
 * write.
 *
 * A MIDDLEWARE, not a check inside each handler. A per-handler check is a
 * thing you can forget to add, and the one you forget is the hole; there is no
 * route here that can answer without passing through this. It sits INSIDE
 * `noStore`, so a 401 still leaves with `cache-control` like everything else.
 *
 * A corpus with no declared principals is OPEN and this is a pass-through —
 * see `domain/model/Principal.ts` for why that is right rather than lax.
 *
 * 401 vs 403 is the real distinction and worth keeping: 401 means "I do not
 * know who you are", 403 means "I know, and no". Collapsing them would tell a
 * reader whose token works to go looking for a better token.
 */
const authorize =
  (config: Config): Middleware =>
  async (c, next) => {
    const access = accessFor(
      config.principals,
      bearerOf(
        fromNullable(
          c.req.headers["authorization"],
        ),
      ),
    );
    if (!mayRead(access)) {
      return err(
        unauthorized(
          access.__tag === "Denied"
            ? access.reason
            : "not a principal of this corpus",
        ),
      );
    }
    // Only a write needs the second question. Deriving it from the METHOD
    // rather than the path means a route added later cannot forget to be
    // covered: if it changes something, it says POST, and it is checked.
    if (
      c.req.method !== "GET" &&
      !mayEdit(access)
    ) {
      return err(
        forbidden(
          "this principal may read this corpus but not change it",
        ),
      );
    }
    return next(c);
  };

// `pipe` takes a fixed chain, so the editor's two routes are added by one step
// that either registers them or hands the app back untouched. A `Web => Web`
// either way, which keeps the pipeline readable and the absence explicit.
// Anything POSTed that no route claims. Its whole job is to exist, so the
// response goes through the middleware rather than around it.
const methodNotHere = (
  c: Context,
): PromisedResult<HttpResponse, HttpError> =>
  Promise.resolve(err(notFound(c.req.path)));

const withEditor =
  (
    ref: IndexRef,
    edit:
      | Readonly<{
          fs: FileSystem;
          writer: FileWriter;
        }>
      | undefined,
  ) =>
  (app: Web): Web =>
    edit === undefined
      ? app
      : pipe(
          app,
          get(
            "/edit/*path",
            editFormHandler(ref),
          ),
          post(
            "/edit/*path",
            editSaveHandler(
              ref,
              edit.fs,
              edit.writer,
            ),
          ),
        );

/**
 * `GET /qfs?path=…&cols=…` — the door from the corpus column's form into
 * generic qfs browsing.
 *
 * A REDIRECT, not a page: an HTML form cannot write itself into the `cols`
 * value, so this route does exactly that one translation — validate the
 * typed path, append it to the carried trail, answer 303 to the trail URL —
 * and the screen the reader lands on IS an address, like every other screen
 * (workaholic:design / modeless-design). The path is untrusted input and
 * goes through `asQfsPath` before it can enter a trail; a refusal is a 400
 * that names the rule, because the person who typed the path reads it.
 */
const qfsEntryHandler = (
  c: Context,
): PromisedResult<HttpResponse, HttpError> => {
  const trail = parseTrail(
    fromNullable(c.req.query["cols"]),
  );
  const raw = c.req.query["path"];
  const path = asQfsPath(
    raw === undefined ? "" : raw.trim(),
  );
  return Promise.resolve(
    isOk(path)
      ? ok(
          textResponse("", statusOf(303), {
            location: trailUrl(
              openFrom(
                trail,
                trail.length - 1,
                qfsStop(path.content),
              ),
            ),
          }),
        )
      : err(
          badRequest(
            path.content.content.message,
          ),
        ),
  );
};

/**
 * `GET /api/documents` — the corpus, filtered, ordered, and paged.
 *
 * Paths only. The source is a separate fetch: listing a large corpus should
 * not ship every byte of it.
 *
 * The query bag is untrusted input and goes through `parseListQuery` before it
 * can address the index — a bad `limit` is a 400 naming the field, not a
 * silent default, so a broken caller learns of its bug
 * (workaholic:implementation / type-driven-design).
 *
 * `count` is retained beside `totalCount` and means what it always did: the
 * whole corpus, ignoring the filter. Dropping it would have broken the health
 * check and any existing reader for no gain.
 */
const listHandler =
  (ref: IndexRef) =>
  (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    // Read the index ONCE. Everything below works on that value, so a reload
    // mid-request cannot tear this response.
    const index: Index = ref.current();
    const query = parseListQuery(c.req.query);
    if (!isOk(query)) {
      return Promise.resolve(
        err(
          badRequest(
            query.content.content.message,
          ),
        ),
      );
    }
    const page = listCollection(
      index,
      query.content,
    );
    return Promise.resolve(
      ok(
        json({
          count: documentCount(index),
          totalCount: page.totalCount,
          limit: page.limit,
          offset: page.offset,
          documents: page.contents.map((d) => ({
            path: documentPathString(
              d.content.path,
            ),
          })),
        }),
      ),
    );
  };

/**
 * `GET /api/documents/*path` — one document's source.
 *
 * The captured path is raw user input. It goes through `asDocumentPath`
 * before it can address the index, so a traversing or absolute path is
 * rejected at the boundary rather than read from disk
 * (workaholic:implementation / type-driven-design). `getDocument` will not
 * accept a bare string anyway — the brand is the second lock.
 */
const documentHandler =
  (ref: IndexRef) =>
  (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    const index: Index = ref.current();
    const raw = c.req.params["path"];
    const path = asDocumentPath(
      raw === undefined ? "" : raw,
    );
    if (!isOk(path)) {
      return Promise.resolve(
        err(notFound(`/api/documents/${raw}`)),
      );
    }
    const doc = getDocument(index, path.content);
    return Promise.resolve(
      doc.__tag === "Some"
        ? ok(
            json({
              path: documentPathString(
                doc.content.content.path,
              ),
              // `null`, not an omitted key: a document whose front matter
              // the producer declined is a document with no front matter
              // AS FAR AS THIS API IS CONCERNED, and a consumer should not
              // have to guess whether the key is missing because the fence was
              // absent or because we dropped it. `/api/errors` says which.
              frontMatter: matchOption<
                FrontMatter,
                FrontMatter | null
              >(
                () => null,
                (fields) => fields,
              )(doc.content.content.frontMatter),
              source: doc.content.content.source,
            }),
          )
        : err(notFound(`/api/documents/${raw}`)),
    );
  };

/**
 * `GET /api/errors` — the documents the scan could not read.
 *
 * A first-class endpoint because the corpus's failures are part of the model,
 * not a log line: a document that vanished or would not parse is a fact a
 * consumer should be able to see.
 */
const errorsHandler =
  (ref: IndexRef) =>
  (): PromisedResult<HttpResponse, HttpError> => {
    const index: Index = ref.current();
    return Promise.resolve(
      ok(
        json({
          errors: indexErrors(index).map((e) => ({
            path: e.content.path,
            message: e.content.message,
          })),
        }),
      ),
    );
  };

/** `GET /api/health` — the corpus at a glance. */
const healthHandler =
  (ref: IndexRef) =>
  (): PromisedResult<HttpResponse, HttpError> => {
    const index: Index = ref.current();
    return Promise.resolve(
      ok(
        json({
          documentCount: documentCount(index),
          errorCount: indexErrors(index).length,
        }),
      ),
    );
  };

/**
 * The API, over a live index.
 *
 * Takes the `IndexRef` rather than an `Index` so the routes see the CURRENT
 * corpus after a hot reload — while each individual request still reads one
 * consistent value.
 */
export const api = (
  ref: IndexRef,
  edit?: Readonly<{
    fs: FileSystem;
    writer: FileWriter;
  }>,
  config: Config = defaultConfig,
  runner?: ResourceRunner,
): Web =>
  pipe(
    web(),
    // Outermost: every response, success or error, leaves with no-store.
    use(noStore),
    // Then: is this request allowed at all. Inside noStore so a 401 still
    // carries cache-control; outside every route so none can bypass it.
    use(authorize(config)),
    // The root a person actually types. Registered first so nothing shadows it.
    get("/", columnsHandler(ref, config, runner)),
    // The canonical trail address (docs/adr/0007). The wildcard exists to
    // CLAIM the path space — the handler reads the raw `c.req.path`, because
    // the router percent-decodes its capture and one decode too many would
    // let `%2C` forge the segment separator. Registered before the document
    // catch-all, which this deliberately shadows: a corpus directory named
    // `resolve/` is not browsable at `/resolve/…` (the ADR records the
    // trade).
    get(
      "/resolve/*trail",
      resolveHandler(ref, config, runner),
    ),
    // The form's translation step — see qfsEntryHandler. Registered with the
    // static routes so the document catch-all cannot swallow it.
    get("/qfs", qfsEntryHandler),
    get("/api/health", healthHandler(ref)),
    get("/api/errors", errorsHandler(ref)),
    get("/api/documents", listHandler(ref)),
    // The editor, mounted only when a writer was supplied. `api(ref)` with no
    // writer is a READ-ONLY server, and that is not a formality: it is what
    // lets every spec that does not test editing prove, by construction, that
    // it cannot reach a disk. The capability IS the argument.
    withEditor(ref, edit),
    // The wildcard is registered after the static routes: a document path
    // contains slashes, and lookup picks the first-registered match, so a
    // greedy wildcard ahead of them would swallow /api/documents.
    get(
      "/api/documents/*path",
      documentHandler(ref),
    ),
    // The catch-all, registered LAST so it shadows nothing: every remaining
    // path is a candidate DOCUMENT, server-rendered.
    //
    // It is also the 404, and deliberately the same handler. A path that is
    // not a valid document path, or names nothing in the index, is a miss —
    // `/foo` is not markdown and `/docs/gone.md` is not in the corpus, and
    // both answer 404 through this one route rather than through two that
    // could disagree.
    //
    // Having a route at all is not decoration: plgg-server answers an
    // UNMATCHED path with its own 404, and that response never passes through
    // global `use()` middleware — so a live curl of /foo came back with no
    // `cache-control` at all, quietly violating docs/adr/0003 ("every
    // response"). Giving every path a route means every response leaves
    // through the noStore middleware.
    get("/*path", documentPageHandler(ref)),
    // The same catch-all, for POST — and for the same reason the GET one
    // exists. plgg-server answers a path it has no route for with its OWN
    // response, which never passes through global `use()` middleware: on a
    // server built with no writer, a POST came back as a bare
    // `MethodNotAllowed` carrying no `cache-control` at all. That is the exact
    // ADR 0003 escape a live curl caught on 404s once already, wearing a
    // different method. Giving POST a route means every response leaves
    // through `noStore`.
    //
    // Registered AFTER the editor's POST, so where an editor exists it wins;
    // where it does not, a write attempt is a plain 404.
    post("/*path", methodNotHere),
  );
