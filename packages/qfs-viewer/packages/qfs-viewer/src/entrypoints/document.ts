// `GET /<path>` — one document, server-rendered.
//
// An entrypoint, so plgg-md and plgg-view may be named here; the domain may
// not. What lives here is the WIRING — resolve a route against the index,
// hand the source to plgg-md, wrap the result in a page. The two decisions
// with actual content, `formatOrdinal` and the route→path mapping, are pure
// and live in `domain/` where they are tested.
//
// The route resolves against the INDEX, never the filesystem
// (ticket …004236 step 6). The index is the serving model; probing the disk
// per request would reintroduce the very inconsistency the immutable index and
// its atomic swap exist to prevent, and would answer 200 for a file the
// watcher had already removed.
import {
  isOk,
  ok,
  err,
  type PromisedResult,
} from "plgg";
import {
  renderMarkdownWithOptions,
  renderOptions,
  plainHighlighter,
  identityResolver,
  type HeadingParts,
} from "plgg-md";
import {
  pageResponse,
  notFound,
  internalError,
  type Context,
  type HttpResponse,
  type HttpError,
} from "plgg-server";
import {
  type Html,
  h1,
  h2,
  h3,
  h4,
  h5,
  h6,
  text,
  id_,
} from "plgg-view";
import { formatOrdinal } from "#qfs-viewer/domain/model/Numbering";
import { getDocument } from "#qfs-viewer/domain/model/Index";
import { type IndexRef } from "#qfs-viewer/domain/usecase/reload";
import {
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

/**
 * Build the heading element for one heading.
 *
 * The number is part of the element's CONTENT, not a CSS `::before`
 * (`workaholic:planning` / `accessibility-first`, and ticket …004236 step 7).
 * A CSS-generated number is invisible to a screen reader, to `curl`, and to
 * the MCP surface this index will grow — all three of which read the same
 * document tree a browser does. The number is the citation, so it has to be in
 * the text.
 *
 * A real `h1`–`h6` for every level, never a styled `div`: the outline IS the
 * structure, and it is what assistive tech and the MCP surface both navigate
 * by. `id` carries plgg-md's already-deduplicated slug, so `#same-1` stays
 * stable across a reload.
 *
 * A literal ladder rather than a lookup, because the six builders have
 * distinct return tags (`h1`…`h6`) — the same reason plgg-md's own
 * `defaultHeading` is written this way.
 */
export const numberedHeading = (
  parts: HeadingParts,
): Html<never> => {
  const attrs = [id_(parts.id)];
  const children = [
    text(`${formatOrdinal(parts.ordinal)} `),
    ...parts.children,
  ];
  return parts.level === 1
    ? h1(attrs, children)
    : parts.level === 2
      ? h2(attrs, children)
      : parts.level === 3
        ? h3(attrs, children)
        : parts.level === 4
          ? h4(attrs, children)
          : parts.level === 5
            ? h5(attrs, children)
            : h6(attrs, children);
};

// One options value, built once: the seams are constant, and rebuilding them
// per request would say otherwise. `rawHtml` stays off — the corpus is other
// people's markdown, so raw HTML passthrough is an injection surface we have
// no reason to open (`workaholic:design` / `security`).
export const RENDER_OPTIONS = {
  ...renderOptions(
    plainHighlighter,
    identityResolver,
  ),
  decorateHeading: numberedHeading,
};

/**
 * `GET /<path>` — render the document at `path`, or 404.
 *
 * The route carries the document's repo-relative path directly
 * (`/docs/adr/0001-npm-only.md`), so the URL alone says which document is on
 * screen and a reload lands on the same place — the navigable state lives in
 * the URL, not in a session (`workaholic:design` / `modeless-design`, and
 * ticket …004236 step 9). That is the foundation the column-accretion UI
 * builds on, and parking it anywhere else would foreclose that.
 */
export const documentPageHandler =
  (ref: IndexRef) =>
  (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    // Read the index ONCE: a reload mid-render cannot tear this page.
    const index = ref.current();
    const raw = c.req.params["path"];
    const path = asDocumentPath(
      raw === undefined ? "" : raw,
    );
    if (!isOk(path)) {
      return Promise.resolve(
        err(notFound(`/${raw}`)),
      );
    }
    const doc = getDocument(index, path.content);
    if (doc.__tag === "None") {
      return Promise.resolve(
        err(notFound(`/${raw}`)),
      );
    }
    const found = doc.content;
    const rendered = renderMarkdownWithOptions(
      RENDER_OPTIONS,
    )(found.content.source);
    // A document that will not render is a 500, not a 404: the corpus HAS it,
    // and telling a reader it does not exist would send them looking for a
    // missing file instead of a broken one. plgg-md is total, so this is the
    // unterminated-fence class of failure, which /api/errors already names.
    return Promise.resolve(
      isOk(rendered)
        ? ok(
            pageResponse({
              title: documentPathString(
                found.content.path,
              ),
              root: rendered.content.body,
            }),
          )
        : err(
            internalError(
              `could not render ${documentPathString(found.content.path)}: ${rendered.content.content.message}`,
            ),
          ),
    );
  };
