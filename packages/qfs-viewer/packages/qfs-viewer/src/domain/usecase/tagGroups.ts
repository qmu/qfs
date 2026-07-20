// Tag groups: the dimensions the corpus can be navigated by.
//
// The mission's non-tree claim rests here. "A ticket is *about* a package, *of*
// a kind, *from* a mission, and the tree can express only one of those" — a
// tag group is one of those dimensions (`type`, `layer`, `mission`), and its
// variations are the values documents actually carry.
//
// THE GROUPS ARE DISCOVERED, NOT DECLARED, and that is a deliberate reading of
// the mission. Its Acceptance says "front-matter tagging with declared groups
// and variations", and a config file declaring them is a later item ("arbitrary
// structured configuration file drives layout and classification"). Deriving
// them from the corpus is what makes the facet work TODAY, on a repository
// with no config, which is the product's whole premise: `npx qfs-viewer` at
// a repository root, no build step, no central configuration. When the config
// item lands it constrains and orders these groups; it does not replace them,
// because a corpus will always carry keys nobody declared and hiding those
// would make the browser lie about what it holds.
import {
  type SoftStr,
  matchOption,
  none,
} from "plgg";
import {
  type Document,
  type FrontMatter,
} from "#qfs-viewer/domain/model/Document";
import {
  type Config,
  defaultConfig,
} from "#qfs-viewer/domain/model/Config";

/**
 * One dimension of the corpus, and the variations documents carry along it.
 *
 * `counts` is part of the value because a facet without counts is a guess: a
 * link to a variation that matches nothing is a dead end the reader only finds
 * by clicking it.
 */
export type TagGroup = Readonly<{
  key: SoftStr;
  /** What the facet heading reads. The key itself unless a config renamed it. */
  label: SoftStr;
  values: ReadonlyArray<SoftStr>;
  counts: Readonly<Record<string, number>>;
}>;

// Front matter is already folded plain data (domain/model/Document.ts) — the
// shape both producers share — so "no front matter" is the only case to fold
// away here.
const fieldsOf = (doc: Document): FrontMatter =>
  matchOption<FrontMatter, FrontMatter>(
    () => ({}),
    (fields) => fields,
  )(doc.content.frontMatter);

// A scalar is one variation. A sequence is several — that is the tag-group
// read: `layer: [Domain, Infrastructure]` puts one document on two variations
// of one dimension, which a directory cannot do.
//
// Anything else is not a variation. A one-level YAML map under a key is a
// structured value, not a tag, and flattening it into facet links would invent
// dimensions nobody wrote.
const variationsOf = (
  value: unknown,
): ReadonlyArray<SoftStr> =>
  typeof value === "string"
    ? [value]
    : typeof value === "number" ||
        typeof value === "boolean"
      ? [String(value)]
      : Array.isArray(value)
        ? value.flatMap((item) =>
            variationsOf(item),
          )
        : [];

// Keys whose values are identifiers rather than dimensions. Faceting on
// `created_at` yields one variation per document — a list of every timestamp,
// which is noise wearing a facet's clothes.
//
// Named rather than inferred ("a key whose variations are all unique is not a
// dimension") because that heuristic would hide a real dimension the moment a
// corpus had one document per value, which is exactly what a NEW repository
// looks like.
const NOT_DIMENSIONS: ReadonlyArray<string> = [
  "created_at",
  "updated_at",
  "commit_hash",
  // `depends_on` carries TICKET FILENAMES —
  // `20260715004235-markdown-scanner-and-frontmatter-index.md`. That is an
  // identifier by every test this list applies: it names one document rather
  // than describing a kind of document, so its "variations" are primary keys
  // and its facet is a list of links each reaching one thing.
  //
  // It earns its place here from the screen rather than from theory: on this
  // corpus it rendered three chips of wrapped filenames, taking more vertical
  // room than every real dimension combined and pushing the document list off
  // the page. Belongs with `commit_hash`, not with `layer`.
  "depends_on",
  "title",
  "url",
  "endpoint",
  "command",
  "slug",
];

// A config's `hide` REPLACES the built-in list rather than adding to it, so a
// corpus whose `title` really IS a tag can say so. The built-in list is the
// default for the no-config case, not a floor a repository cannot argue with.
const hiddenKeys = (
  config: Config,
): ReadonlyArray<string> =>
  matchOption<
    ReadonlyArray<SoftStr>,
    ReadonlyArray<string>
  >(
    () => NOT_DIMENSIONS,
    (declared) => declared,
  )(config.hide);

/**
 * Every dimension a SET OF DOCUMENTS carries, with its variations.
 *
 * Takes the documents, not the index, and that is the fix for a real bug: the
 * facet counts are only true of the set they were counted over. Handed the
 * whole index while the list was filtered, a facet read `enhancement (5)` next
 * to a list of 3 — so the number promised five documents that clicking it
 * would never produce, because the click ANDs with the filter already on. The
 * count and the list have to be counted over the same set or the count is a
 * lie.
 *
 * Groups are ordered by how many documents they reach (most first), then by
 * key, so the facet a reader is most likely to want is at the top and the
 * order does not shuffle between reloads. Variations are ordered by count for
 * the same reason.
 *
 * Documents with no front matter contribute nothing and are not an error:
 * 19 of this repository's 28 documents have no fence at all (the ADRs, the
 * READMEs), and a knowledge base does not require one.
 */
export const tagGroupsOf = (
  documents: ReadonlyArray<Document>,
  config: Config = defaultConfig,
): ReadonlyArray<TagGroup> => {
  const hidden = hiddenKeys(config);
  const declaredKeys = config.tagGroups.map(
    (g) => g.key,
  );
  const counts = new Map<
    string,
    Map<string, number>
  >();
  for (const doc of documents) {
    for (const [key, value] of Object.entries(
      fieldsOf(doc),
    )) {
      // A declared group outranks the hide list: naming a key in BOTH is a
      // contradiction, and the one that says "show this" is the one written
      // with intent.
      if (
        hidden.includes(key) &&
        !declaredKeys.includes(key)
      ) {
        continue;
      }
      const variations = variationsOf(value);
      if (variations.length === 0) {
        continue;
      }
      const byValue =
        counts.get(key) ??
        new Map<string, number>();
      for (const variation of variations) {
        byValue.set(
          variation,
          (byValue.get(variation) ?? 0) + 1,
        );
      }
      counts.set(key, byValue);
    }
  }
  const reach = (byValue: Map<string, number>) =>
    [...byValue.values()].reduce(
      (a, b) => a + b,
      0,
    );
  // A declared group's position is its position in the config; everything
  // discovered follows, ordered by reach. So a config decides what a reader
  // sees FIRST without deciding what they are allowed to see at all.
  const rank = (key: string): number => {
    const at = declaredKeys.indexOf(key);
    // A discovered key ranks after every declared one; among themselves they
    // tie, and the reach comparison below breaks it.
    return at === -1 ? declaredKeys.length : at;
  };
  const declaredOf = (key: string) =>
    config.tagGroups.find((g) => g.key === key);

  return [...counts.entries()]
    .sort(([ak, av], [bk, bv]) => {
      const ra = rank(ak);
      const rb = rank(bk);
      if (ra !== rb) {
        return ra - rb;
      }
      // Both discovered: most-reaching first, then by key so the order cannot
      // shuffle between reloads.
      return reach(bv) === reach(av)
        ? ak < bk
          ? -1
          : 1
        : reach(bv) - reach(av);
    })
    .map(([key, byValue]) => {
      const declaration = declaredOf(key);
      const discovered = [...byValue.entries()]
        .sort(([av, ac], [bv, bc]) =>
          bc === ac
            ? av < bv
              ? -1
              : 1
            : bc - ac,
        )
        .map(([v]) => v);
      // A declared `variations` list FIXES the order and the membership: it is
      // a taxonomy someone wrote down, so a variation with no documents still
      // shows (with a count of 0 — an empty shelf is information), and one the
      // corpus carries but the taxonomy omits is appended rather than dropped,
      // because a document that exists is not made to disappear by a config
      // that forgot it.
      const values = matchOption<
        ReadonlyArray<SoftStr>,
        ReadonlyArray<SoftStr>
      >(
        () => discovered,
        (fixed) => [
          ...fixed,
          ...discovered.filter(
            (v) => !fixed.includes(v),
          ),
        ],
      )(
        declaration === undefined
          ? none()
          : declaration.variations,
      );
      const label = matchOption<SoftStr, SoftStr>(
        () => key,
        (l) => l,
      )(
        declaration === undefined
          ? none()
          : declaration.label,
      );
      return {
        key,
        label,
        values,
        counts: Object.fromEntries(byValue),
      };
    });
};
