// The project vocabulary, as types.
//
// One concept, one word — used identically here, in the REST
// paths, and in the MCP tool names (workaholic:planning /
// terminology). These four all carry text, so nothing but a
// brand stops a Route being passed where a DocumentPath is
// meant. Each is a `refinedBrand`, so a value only exists
// having passed its predicate at a boundary
// (workaholic:implementation / type-driven-design), and the
// brand is a `Box` rather than an `as`-cast intersection —
// the repo forbids `as` outright.
//
//   DocumentPath   — where a document lives, repo-relative
//                    ("docs/adr/0001-npm-only.md")
//   DocumentSlug   — a document's stable identity ("0001-npm-only")
//   HeadingAnchor  — a citable point inside a document ("goal")
//   Route          — what a reader's URL asks for
//                    ("/docs/adr/0001-npm-only")
//
// A *scan* walks the roots and yields documents; the *index*
// holds them; *front matter* is the tagged head of a document.
// Those three name behavior, not data, so they live on the
// functions in ../usecase, not here.
import {
  type Box,
  type Option,
  type InvalidError,
  invalidError,
  refinedBrand,
  isSoftStr,
  some,
  none,
} from "plgg";

// A path traverses if any SEGMENT is "..". Checked on the
// segments, not the raw string: a substring test would reject
// the legitimate "..foo.md" and miss the traversing
// "a/../../b".
const traverses = (value: string): boolean =>
  value.split("/").some((s) => s === "..");

const hasEmptySegment = (
  value: string,
): boolean =>
  value.split("/").some((s) => s === "");

// Case-insensitive: the corpus is other people's markdown, and `README.MD`
// exists in the wild. This must agree with `isDocumentFile` in
// domain/model/Scan.ts — when the two disagreed, a `.MD` file was walked but
// then rejected here, so it vanished into the scan's collected errors instead
// of being indexed. The scan spec pins the agreement.
const isMarkdown = (value: string): boolean =>
  value.toLowerCase().endsWith(".md");

// Shared by DocumentSlug and HeadingAnchor: an anchor is a
// slug scoped to a document, and the two must agree or a
// citation cannot round-trip.
const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * Where a document lives, relative to the scanned root.
 * Never absolute, never escaping the root, always markdown.
 */
export type DocumentPath = Box<
  "DocumentPath",
  string
>;

const documentPath = refinedBrand<
  "DocumentPath",
  string,
  InvalidError
>(
  "DocumentPath",
  (v): v is string =>
    isSoftStr(v) &&
    v.length > 0 &&
    !v.startsWith("/") &&
    isMarkdown(v) &&
    !traverses(v) &&
    !hasEmptySegment(v),
  (v) =>
    invalidError({
      message: `not a DocumentPath (want a relative, non-traversing .md path, got ${JSON.stringify(v)})`,
    }),
);

/** Type guard for {@link DocumentPath}. */
export const isDocumentPath = documentPath.is;

/**
 * Validates an unknown value into a {@link DocumentPath} at a
 * boundary.
 */
export const asDocumentPath = documentPath.as;

/** The underlying string of a {@link DocumentPath}. */
export const documentPathString =
  documentPath.unwrap;

/** A document's stable identity, used to cite it. */
export type DocumentSlug = Box<
  "DocumentSlug",
  string
>;

const documentSlug = refinedBrand<
  "DocumentSlug",
  string,
  InvalidError
>(
  "DocumentSlug",
  (v): v is string =>
    isSoftStr(v) && SLUG.test(v),
  (v) =>
    invalidError({
      message: `not a DocumentSlug (want lowercase alphanumerics separated by single hyphens, got ${JSON.stringify(v)})`,
    }),
);

/** Type guard for {@link DocumentSlug}. */
export const isDocumentSlug = documentSlug.is;

/**
 * Validates an unknown value into a {@link DocumentSlug} at a
 * boundary.
 */
export const asDocumentSlug = documentSlug.as;

/** The underlying string of a {@link DocumentSlug}. */
export const documentSlugString =
  documentSlug.unwrap;

/**
 * A citable point inside a document. Shares the slug grammar:
 * an anchor is a slug scoped to a document.
 */
export type HeadingAnchor = Box<
  "HeadingAnchor",
  string
>;

const headingAnchor = refinedBrand<
  "HeadingAnchor",
  string,
  InvalidError
>(
  "HeadingAnchor",
  (v): v is string =>
    isSoftStr(v) && SLUG.test(v),
  (v) =>
    invalidError({
      message: `not a HeadingAnchor (want lowercase alphanumerics separated by single hyphens, got ${JSON.stringify(v)})`,
    }),
);

/** Type guard for {@link HeadingAnchor}. */
export const isHeadingAnchor = headingAnchor.is;

/**
 * Validates an unknown value into a {@link HeadingAnchor} at a
 * boundary.
 */
export const asHeadingAnchor = headingAnchor.as;

/** The underlying string of a {@link HeadingAnchor}. */
export const headingAnchorString =
  headingAnchor.unwrap;

/**
 * What a reader's URL asks for. Leading-slashed, no trailing
 * slash (except the root "/"), no traversal.
 */
export type Route = Box<"Route", string>;

const route = refinedBrand<
  "Route",
  string,
  InvalidError
>(
  "Route",
  (v): v is string =>
    isSoftStr(v) &&
    v.startsWith("/") &&
    !traverses(v) &&
    (v === "/" ||
      (!v.endsWith("/") &&
        !hasEmptySegment(v.slice(1)))),
  (v) =>
    invalidError({
      message: `not a Route (want a leading-slashed, non-traversing path with no trailing slash, got ${JSON.stringify(v)})`,
    }),
);

/** Type guard for {@link Route}. */
export const isRoute = route.is;

/**
 * Validates an unknown value into a {@link Route} at a
 * boundary.
 */
export const asRoute = route.as;

/** The underlying string of a {@link Route}. */
export const routeString = route.unwrap;

// Resolve `.` and `..` against the segments, and report an escape rather than
// clamping it. A link that climbs above the root is a broken link in the
// document, not a link to the root — silently clamping it would answer with
// some unrelated document and call that success.
const normalizeSegments = (
  segments: ReadonlyArray<string>,
): Option<ReadonlyArray<string>> => {
  const out: Array<string> = [];
  for (const segment of segments) {
    if (segment === "." || segment === "") {
      continue;
    }
    if (segment === "..") {
      if (out.length === 0) {
        return none();
      }
      out.pop();
      continue;
    }
    out.push(segment);
  }
  return some(out);
};

/**
 * Resolve a link written INSIDE `from` into a repo-relative
 * {@link DocumentPath}.
 *
 * Markdown links are relative to the file that carries them: `docs/adr/index.md`
 * writing `](0001-npm-only.md)` means `docs/adr/0001-npm-only.md`, not a file
 * at the repository root. Treating the target as already-root-relative is the
 * bug this exists to prevent — it resolved every ADR-index link to a
 * nonexistent root-level document, and every one of them opened a column
 * saying "not in the corpus".
 *
 * A leading `/` means root-relative, which is how a document addresses the
 * corpus rather than its neighbour.
 *
 * `None` for anything that is not a document in this corpus: an external URL,
 * a `mailto:`, a bare anchor, or a path that climbs above the root. The caller
 * leaves those exactly as the author wrote them — rewriting
 * `https://example.com` into a column would be a bug that ate the web.
 */
export const resolveRelativePath = (
  from: DocumentPath,
  target: string,
): Option<DocumentPath> => {
  // A scheme means it is not ours. Checked before anything else, because
  // `https://x/y.md` would otherwise normalize into a plausible-looking path.
  if (/^[a-z][a-z0-9+.-]*:/i.test(target)) {
    return none();
  }
  const rooted = target.startsWith("/");
  const base = rooted
    ? []
    : documentPathString(from)
        .split("/")
        .slice(0, -1);
  const normalized = normalizeSegments([
    ...base,
    ...target.split("/"),
  ]);
  if (normalized.__tag === "None") {
    return none();
  }
  const resolved = asDocumentPath(
    normalized.content.join("/"),
  );
  return resolved.__tag === "Ok"
    ? some(resolved.content)
    : none();
};
