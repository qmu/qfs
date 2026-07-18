import {
  type SoftStr,
  type Option,
  fromNullable,
  matchOption,
  getOr,
  pipe,
} from "plgg";
import {
  type Url,
  makeUrl,
} from "plgg-view/client";
import { type Model } from "plggmatic/Schedule/model/Model";

/**
 * The URL-reflected slice of a scheduled model: the
 * chosen root collection, the drill-down path of selected
 * ids, the active keyword text, and the chosen values of
 * the declared query choices (typed query fields, point
 * 4). The derived codec's currency — `parseUrl` yields
 * one, `toUrl` serializes one, and they round-trip.
 * `choices` is optional so a keyword-only slice literal
 * keeps compiling; absent means none.
 */
export type UrlSlice = Readonly<{
  root: Option<SoftStr>;
  path: ReadonlyArray<SoftStr>;
  query: SoftStr;
  choices?: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >;
}>;

/**
 * Folds a {@link Url} into a {@link UrlSlice}. TOTAL: any
 * string yields a valid slice, never throws — a URL is
 * user input (the oracle's standard). Missing `c`/`p`/`q`
 * are `None`/`[]`/`""`. Path segments are `decodeURI`d,
 * so the codec is the exact inverse of {@link toUrl}.
 */
export const parseUrl = (url: Url): UrlSlice => {
  const params = new URLSearchParams(url.search);
  const rawPath = pipe(
    fromNullable(params.get("p")),
    getOr(""),
  );
  return {
    root: fromNullable(params.get("c")),
    path:
      rawPath === ""
        ? []
        : rawPath
            .split("/")
            .map(decodeURIComponent),
    query: pipe(
      fromNullable(params.get("q")),
      getOr(""),
    ),
    // every OTHER param is a candidate declared-choice
    // value; the scheduler validates candidates against
    // the declaration (unknown ids drop, order
    // normalizes), keeping this parser declaration-free
    // and total.
    choices: [...params.entries()].filter(
      ([key]) =>
        key !== "c" && key !== "p" && key !== "q",
    ),
  };
};

/**
 * The `?c=…&p=…/…&q=…` search string for a slice, or `""`
 * when the slice is empty (root only). Canonical: equal
 * slices serialize byte-equal, which is what lets the
 * runtime gate history writes on a string diff. Built as
 * one expression (no imperative push).
 */
const buildSearch = (
  slice: UrlSlice,
): SoftStr => {
  const parts: ReadonlyArray<SoftStr> = [
    ...matchOption<
      SoftStr,
      ReadonlyArray<SoftStr>
    >(
      () => [],
      (r: SoftStr) => [
        `c=${encodeURIComponent(r)}`,
      ],
    )(slice.root),
    ...(slice.path.length > 0
      ? [
          `p=${slice.path
            .map(encodeURIComponent)
            .join("/")}`,
        ]
      : []),
    ...(slice.query !== ""
      ? [`q=${encodeURIComponent(slice.query)}`]
      : []),
    // chosen (non-empty) declared-choice values, in the
    // order the slice carries them — the scheduler keeps
    // that order declared, so equal states stay
    // byte-equal (canonical).
    ...pipe(
      fromNullable(slice.choices),
      getOr<
        ReadonlyArray<readonly [SoftStr, SoftStr]>
      >([]),
    )
      .filter(([, value]) => value !== "")
      .map(
        ([id, value]) =>
          `${encodeURIComponent(id)}=${encodeURIComponent(value)}`,
      ),
  ];
  return parts.length === 0
    ? ""
    : `?${parts.join("&")}`;
};

/**
 * The model→URL projection (inverse of
 * {@link parseUrl}). Serializes only the URL-reflected
 * slice against the model's mount `base`.
 */
export const toUrl = (model: Model): Url =>
  makeUrl(
    model.base,
    buildSearch({
      root: model.root,
      path: model.path,
      query: model.query,
      choices: model.queryChoices,
    }),
  );

/**
 * The href string (`base` + search) a link should point
 * at for a target slice — the renderer seam's currency
 * for drill/truncate navigation (every arrangement is a
 * shareable address, and leaving a level is a link, not a
 * mode switch). Uses the same `buildSearch`, so a link's
 * href and the reflected URL for the same slice are
 * byte-equal.
 */
export const hrefFor = (
  base: SoftStr,
  slice: UrlSlice,
): SoftStr => `${base}${buildSearch(slice)}`;
