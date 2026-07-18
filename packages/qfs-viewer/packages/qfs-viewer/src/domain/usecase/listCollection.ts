// Querying the corpus: filter, order, page.
//
// Pure functions over an index VALUE. The caller reads `ref.current()` once
// and hands the value here, so a reload mid-query cannot tear the answer —
// the property `reload.ts` exists to guarantee, spent here.
//
// NO REVERSE MAPS, deliberately. Ticket …004235 asks for "reverse maps per tag
// dimension", and this is a linear scan instead. The ticket also cites
// `workaholic:design / sacrificial-architecture` — "do NOT pre-optimize the
// on-memory index" — and those two instructions collide. The scan wins: this
// repository's corpus is 28 documents and plgg's is 661, so filtering is
// microseconds either way, while a reverse map is a second representation of
// the truth that every mutation path (`withDocument`, `withoutDocument`,
// `buildIndex`) would have to keep in step — and the index has already shipped
// one bug of exactly that shape, where `errors` fell out of step with
// `documents` on reload. Build the map when a real corpus is slow, and let the
// measurement say so.
import {
  type SoftStr,
  type Option,
  matchOption,
} from "plgg";
import {
  type Document,
  type FrontMatter,
} from "#qfs-viewer/domain/model/Document";
import {
  type Index,
  listDocuments,
} from "#qfs-viewer/domain/model/Index";
import {
  type ListQuery,
  type TagFilter,
} from "#qfs-viewer/domain/model/Query";

/**
 * A page of the corpus.
 *
 * `totalCount` is the count under the SAME filter, before paging — what a
 * pager needs to render "1 of 9". Naming it `totalCount` rather than `total`
 * follows `plgg-cms/model/ListResult.ts`.
 */
export type ListResult = Readonly<{
  contents: ReadonlyArray<Document>;
  totalCount: number;
  limit: number;
  offset: number;
}>;

// A front-matter value, folded to plain data, compared against the text a
// caller sent. Numbers and booleans are compared by their rendered form, so
// `?draft=true` and `?order=2` work without the caller knowing the YAML type.
const scalarMatches = (
  value: unknown,
  want: SoftStr,
): boolean =>
  typeof value === "string"
    ? value === want
    : typeof value === "number" ||
        typeof value === "boolean"
      ? String(value) === want
      : false;

// A sequence matches when ANY item does — this is the tag-group read:
// `layer: [Domain, Infrastructure]` answers to `?layer=Domain`.
const valueMatches = (
  value: unknown,
  want: SoftStr,
): boolean =>
  Array.isArray(value)
    ? value.some((item) =>
        scalarMatches(item, want),
      )
    : scalarMatches(value, want);

/**
 * Whether a document satisfies one front-matter filter.
 *
 * A document with NO front matter matches nothing — the honest answer, and
 * the common case: 19 of this repository's 28 documents have no fence at all
 * (the ADRs, README, CLAUDE.md), and a fence is not something a knowledge base
 * should be required to carry.
 *
 * It is also the answer for a fence plgg-md declines. That was load-bearing
 * under 0.0.2, which refused the flow sequences and empty values every
 * workaholic ticket writes and so left the whole ticket corpus unfacetable;
 * 0.0.3 fixed it upstream. What is still declined is genuine attack surface
 * (`&`, `!!`, `|`, `>`) and should stay that way. Either way the document is
 * listed and served — only faceting is unavailable — and `/api/errors` names
 * each one.
 */
export const matchesTag = (
  doc: Document,
  filter: TagFilter,
): boolean =>
  matchOption<FrontMatter, boolean>(
    () => false,
    (fields) =>
      valueMatches(
        fields[filter.key],
        filter.value,
      ),
  )(doc.content.frontMatter);

// Free text is matched against the raw source, case-insensitively — the whole
// document, front matter included, because a reader searching for a word does
// not care which side of the fence it fell on.
const matchesText = (
  doc: Document,
  q: Option<SoftStr>,
): boolean =>
  matchOption<SoftStr, boolean>(
    () => true,
    (text) =>
      doc.content.source
        .toLowerCase()
        .includes(text.toLowerCase()),
  )(q);

/**
 * Every document the query matches, ordered, WITHOUT paging.
 *
 * Split out of {@link listCollection} because two callers need the matched set
 * itself rather than a page of it — `totalCount` here, and the facet counts in
 * the corpus column, which are only true of the set they were counted over.
 *
 * The column used to approximate this by asking for a page of `maxLimit`. That
 * is right up to 100 documents and silently wrong after: plgg's corpus is 1711,
 * so every facet there would have counted the first hundred matches and
 * reported it as the total. A cap that is invisible until the corpus outgrows
 * it is the kind of bug that ships.
 *
 * Filters are ANDed: every tag must match and the text must be present.
 */
export const matchDocuments = (
  index: Index,
  query: ListQuery,
): ReadonlyArray<Document> => {
  const matched = listDocuments(index).filter(
    (doc) =>
      matchesText(doc, query.q) &&
      query.tags.every((t) => matchesTag(doc, t)),
  );
  // `listDocuments` is already path-ascending, so `desc` is a reverse rather
  // than a second sort.
  return query.orderDir === "asc"
    ? matched
    : [...matched].reverse();
};

/**
 * Filter, order, and page the corpus.
 *
 * An `offset` past the end is an empty page, not an error — a pager walking off
 * the end is asking a reasonable question and gets a reasonable answer.
 */
export const listCollection = (
  index: Index,
  query: ListQuery,
): ListResult => {
  const ordered = matchDocuments(index, query);
  return {
    contents: ordered.slice(
      query.offset,
      query.offset + query.limit,
    ),
    totalCount: ordered.length,
    limit: query.limit,
    offset: query.offset,
  };
};
