// The query vocabulary: what a caller may ask of the index.
//
// Named after `plgg-cms/src/content/Query` deliberately — `ListQuery`,
// `ListResult`, `listCollection`, `getDocument` mean here what they mean there
// (workaholic:planning / terminology). The SHAPE differs where the data does,
// and that divergence is the honest part:
//
//   - plgg-cms orders by `updated_at` | `title`. Neither exists here. The
//     `FileSystem` seam has no mtime (it reads names, directories, and bytes —
//     nothing else), and a title would have to come from the first heading,
//     which is the SSR ticket's job and not yet built. Path is the only stable
//     order this corpus has, so there is no `orderBy` field: a closed set of
//     one is a knob that cannot turn, and this package has already deleted one
//     round of exported-but-uncallable API (`emptyIndex`, `withErrors`).
//     `orderBy` returns when there is a second key to choose.
//   - plgg-cms has no tag facets; this mission is built on them. Any
//     unreserved query parameter is a front-matter filter, so `?type=bugfix`
//     and `?type=bugfix&layer=Domain` read the way a person expects.
//
// Every field is BOUNDED or drawn from a CLOSED set, and the parse REJECTS
// garbage rather than clamping it: a caller who sends `limit=abc` has a bug,
// and silently handing back 20 documents hides it (the rule
// `plgg-cms/model/ListQuery.ts` states and this file keeps).
import {
  type SoftStr,
  type Option,
  type Result,
  type InvalidError,
  invalidError,
  fromNullable,
  matchOption,
  isOk,
  ok,
  err,
  some,
  none,
} from "plgg";

/** Sort direction over the document path. */
export type OrderDir = "asc" | "desc";

/**
 * One front-matter filter: a key and the value it must carry.
 *
 * A document matches when its front matter holds `key`, and that key's value
 * either equals `value` or — for a sequence — contains it. That is what makes
 * `?layer=Domain` select a document whose front matter says
 * `layer: [Domain, Infrastructure]`, which is the mission's tag-group read.
 */
export type TagFilter = Readonly<{
  key: SoftStr;
  value: SoftStr;
}>;

/** A validated list query. */
export type ListQuery = Readonly<{
  limit: number;
  offset: number;
  orderDir: OrderDir;
  /** Free text, matched against the document's source. */
  q: Option<SoftStr>;
  /** Front-matter filters, ALL of which must match. */
  tags: ReadonlyArray<TagFilter>;
}>;

/** Page size when `limit` is absent. */
export const defaultLimit = 20;

/**
 * Hard ceiling on `limit`. An oversized page is an error, not a clamp: the
 * corpus is on memory and a caller asking for 10,000 is telling us something
 * is wrong on their side.
 */
export const maxLimit = 100;

/** The untrusted query-parameter bag (all values strings). */
export type QueryParams = Readonly<
  Record<string, string>
>;

/**
 * The parameters this query surface owns. Everything else in the bag is a
 * front-matter filter — so reserving a new name later would silently capture a
 * corpus that used it as a front-matter key, which is why the set is written
 * down once here rather than scattered through the parse.
 */
const RESERVED: ReadonlyArray<string> = [
  "limit",
  "offset",
  "order",
  "q",
  // `cols` is NAVIGATION, not a filter: it says which columns are open.
  // Leaving it out of this list was a bug you could read off the screen —
  // `?cols=docs/adr/index.md` was taken as "front matter where cols equals
  // docs/adr/index.md", which nothing matches, so opening a single column
  // emptied the document list to `0 of 32` and took every facet with it.
  //
  // The unreserved-means-filter rule is what makes `?type=bugfix` work with
  // no wiring, and this is its cost: every parameter the surfaces add must
  // be declared here, or it silently becomes a filter for a key no document
  // has. That is why the list is one place rather than scattered.
  "cols",
];

const at = (
  params: QueryParams,
  key: SoftStr,
): Option<SoftStr> => fromNullable(params[key]);

const parseIntIn =
  (field: SoftStr, lo: number, hi: number) =>
  (
    raw: SoftStr,
  ): Result<number, InvalidError> => {
    // The TEXT is checked before it is converted: `Number("")` is 0 and
    // `Number(" 1 ")` is 1, so a bare `Number(raw)` would let two malformed
    // parameters through as plausible values.
    const n = /^\d+$/.test(raw)
      ? Number(raw)
      : Number.NaN;
    return Number.isInteger(n) &&
      n >= lo &&
      n <= hi
      ? ok(n)
      : err(
          invalidError({
            message: `${field} must be an integer in [${lo}, ${hi}], got ${JSON.stringify(raw)}`,
          }),
        );
  };

const parseOrderDir = (
  raw: SoftStr,
): Result<OrderDir, InvalidError> =>
  raw === "asc" || raw === "desc"
    ? ok(raw)
    : err(
        invalidError({
          message: `order must be 'asc'|'desc', got ${JSON.stringify(raw)}`,
        }),
      );

// An absent parameter takes the default; a present one must parse. `None` is
// not a failure — omitting `limit` is how most callers ask.
const orDefault =
  <T>(fallback: T) =>
  (
    parse: (
      raw: SoftStr,
    ) => Result<T, InvalidError>,
  ) =>
  (o: Option<SoftStr>): Result<T, InvalidError> =>
    matchOption<SoftStr, Result<T, InvalidError>>(
      () => ok(fallback),
      parse,
    )(o);

// Every unreserved parameter is a front-matter filter. Order is preserved so
// an error message can name them back in the order they were sent.
const tagsOf = (
  params: QueryParams,
): ReadonlyArray<TagFilter> =>
  Object.entries(params)
    .filter(([key]) => !RESERVED.includes(key))
    .map(([key, value]) => ({ key, value }));

/**
 * Validates an untrusted query bag into a {@link ListQuery}.
 *
 * The one boundary this vocabulary has: everything inward of it is typed, so
 * `listCollection` never sees a string it must interpret
 * (workaholic:implementation / type-driven-design).
 */
export const parseListQuery = (
  params: QueryParams,
): Result<ListQuery, InvalidError> => {
  const limit = orDefault(defaultLimit)(
    parseIntIn("limit", 1, maxLimit),
  )(at(params, "limit"));
  if (!isOk(limit)) {
    return err(limit.content);
  }
  const offset = orDefault(0)(
    parseIntIn(
      "offset",
      0,
      Number.MAX_SAFE_INTEGER,
    ),
  )(at(params, "offset"));
  if (!isOk(offset)) {
    return err(offset.content);
  }
  const orderDir = orDefault<OrderDir>("asc")(
    parseOrderDir,
  )(at(params, "order"));
  if (!isOk(orderDir)) {
    return err(orderDir.content);
  }
  // `q=` with an empty value is a caller who filtered on nothing; treat it as
  // absent rather than as a substring every document contains.
  const rawQ = at(params, "q");
  const q =
    rawQ.__tag === "Some" &&
    rawQ.content.length > 0
      ? some(rawQ.content)
      : none();
  return ok({
    limit: limit.content,
    offset: offset.content,
    orderDir: orderDir.content,
    q,
    tags: tagsOf(params),
  });
};
