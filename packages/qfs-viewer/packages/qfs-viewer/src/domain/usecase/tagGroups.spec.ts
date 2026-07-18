import {
  test,
  check,
  all,
  toBe,
  toEqual,
  toHaveLength,
} from "plgg-test";
import { fakeFileSystem } from "#qfs-viewer/testkit/fakeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import { listDocuments } from "#qfs-viewer/domain/model/Index";
import { tagGroupsOf } from "#qfs-viewer/domain/usecase/tagGroups";
import { asConfig } from "#qfs-viewer/domain/model/Config";

const fm = (
  ...lines: ReadonlyArray<string>
): string =>
  ["---", ...lines, "---", "# body", ""].join(
    "\n",
  );

const groups = (
  files: Readonly<Record<string, string>>,
) =>
  tagGroupsOf(
    listDocuments(scan(fakeFileSystem(files))),
  );

const keys = (
  files: Readonly<Record<string, string>>,
) => groups(files).map((g) => g.key);

test("a scalar front-matter field is a dimension with one variation", () =>
  check(
    groups({ "a.md": fm("type: bugfix") }),
    toEqual([
      {
        key: "type",
        label: "type",
        values: ["bugfix"],
        counts: { bugfix: 1 },
      },
    ]),
  ));

// The non-tree claim, in one assertion: a sequence puts ONE document on TWO
// variations of ONE dimension. A directory cannot do this — that is the whole
// reason the corpus needs a facet.
test("a sequence puts one document on several variations of one dimension", () =>
  check(
    groups({
      "a.md": fm(
        "layer: [Domain, Infrastructure]",
      ),
    }),
    toEqual([
      {
        key: "layer",
        label: "layer",
        values: ["Domain", "Infrastructure"],
        counts: { Domain: 1, Infrastructure: 1 },
      },
    ]),
  ));

test("numbers and booleans are variations, rendered as their text", () =>
  check(
    groups({
      "a.md": fm("draft: true", "order: 2"),
    }).map((g) => [g.key, g.values]),
    toEqual([
      ["draft", ["true"]],
      ["order", ["2"]],
    ]),
  ));

test("variations count across documents", () =>
  check(
    groups({
      "a.md": fm("type: bugfix"),
      "b.md": fm("type: bugfix"),
      "c.md": fm("type: enhancement"),
    }),
    toEqual([
      {
        key: "type",
        label: "type",
        values: ["bugfix", "enhancement"],
        counts: { bugfix: 2, enhancement: 1 },
      },
    ]),
  ));

// Counts are part of the value because a facet without them is a guess: a
// link to a variation that matches nothing is a dead end found by clicking.
test("a variation's count is the documents that carry it", () =>
  check(
    groups({
      "a.md": fm("layer: [Domain, UX]"),
      "b.md": fm("layer: [Domain]"),
    })[0]?.counts,
    toEqual({ Domain: 2, UX: 1 }),
  ));

// Faceting on a timestamp yields one variation per document — a list of every
// value, which is noise wearing a facet's clothes.
test("identifier-ish keys are not dimensions", () =>
  check(
    keys({
      "a.md": fm(
        "created_at: 2026-07-15T00:00:00+09:00",
        "commit_hash: abc123",
        "title: Something",
        // `depends_on` names ONE document rather than describing a kind of
        // one, so its variations are primary keys. On the real corpus it
        // rendered as three chips of wrapped ticket filenames and pushed the
        // document list off the page — an identifier wearing a facet's
        // clothes, which is exactly what this list is for.
        "depends_on: [20260715004235-markdown-scanner.md]",
        "type: bugfix",
      ),
    }),
    toEqual(["type"]),
  ));

// A one-level map under a key is a structured value, not a tag. Flattening it
// into facet links would invent dimensions nobody wrote.
test("a nested map is a value, not a dimension", () =>
  check(
    keys({
      "a.md": fm(
        "meta:",
        "  owner: a",
        "  state: draft",
        "type: bugfix",
      ),
    }),
    toEqual(["type"]),
  ));

// 19 of this repository's 28 documents have no fence at all. A knowledge base
// does not require one, so contributing nothing is the right answer.
test("documents with no front matter contribute nothing and are not an error", () =>
  all([
    check(
      groups({ "a.md": "# just a body\n" }),
      toEqual([]),
    ),
    check(
      groups({
        "a.md": "# no fence\n",
        "b.md": fm("type: bugfix"),
      })[0]?.counts,
      toEqual({ bugfix: 1 }),
    ),
  ]));

// A fence the subset declines carries `None`, so it cannot be faceted — the
// document is still listed and served.
test("a document whose fence plgg-md declines contributes nothing", () =>
  check(
    groups({
      "a.md": fm("foo: &anchor bar"),
      "b.md": fm("type: bugfix"),
    }),
    toEqual([
      {
        key: "type",
        label: "type",
        values: ["bugfix"],
        counts: { bugfix: 1 },
      },
    ]),
  ));

test("an empty corpus has no dimensions", () =>
  check(groups({}), toHaveLength(0)));

// Ordered by reach so the facet a reader most likely wants is at the top, and
// so the order does not shuffle between reloads.
test("dimensions are ordered by how many documents they reach", () =>
  check(
    keys({
      "a.md": fm("rare: x", "common: 1"),
      "b.md": fm("common: 2"),
      "c.md": fm("common: 3"),
    }),
    toEqual(["common", "rare"]),
  ));

test("dimensions with equal reach are ordered by key, so the list is stable", () =>
  check(
    keys({
      "a.md": fm("zeta: x", "alpha: y"),
    }),
    toEqual(["alpha", "zeta"]),
  ));

test("variations are ordered by count, then by value", () =>
  check(
    groups({
      "a.md": fm("type: [b, a, common]"),
      "b.md": fm("type: [common]"),
    })[0]?.values,
    toEqual(["common", "a", "b"]),
  ));

// An empty value parses (since plgg-md 0.0.3) but names no variation, so it is
// not a dimension of one blank tag.
test("an empty front-matter value is not a variation", () =>
  check(
    keys({
      "a.md": fm("effort:", "type: bugfix"),
    }),
    toEqual(["type"]),
  ));

// ---- config-driven ----

const cfg = (raw: unknown) => {
  const c = asConfig(raw);
  if (c.__tag === "Err") {
    throw new Error(c.content.content.message);
  }
  return c.content;
};

const grouped = (
  files: Readonly<Record<string, string>>,
  config: unknown,
) =>
  tagGroupsOf(
    listDocuments(scan(fakeFileSystem(files))),
    cfg(config),
  );

const FACETED = {
  "a.md": fm(
    "type: bugfix",
    "layer: [Domain]",
    "author: a",
  ),
  "b.md": fm(
    "type: enhancement",
    "layer: [UX]",
    "author: a",
  ),
};

// A declared group is ordered FIRST; discovery still decides the rest.
test("declared groups lead, in the order the config wrote them", () =>
  check(
    grouped(FACETED, {
      tagGroups: [
        { key: "type" },
        { key: "layer" },
      ],
    }).map((g) => g.key),
    toEqual(["type", "layer", "author"]),
  ));

// The keys nobody thought to declare are exactly the ones worth seeing, so a
// config that mentions one dimension does not hide the others.
test("an undeclared dimension still appears, after the declared ones", () =>
  check(
    grouped(FACETED, {
      tagGroups: [{ key: "layer" }],
    }).map((g) => g.key),
    toEqual(["layer", "author", "type"]),
  ));

test("a label renames the facet heading without touching the key", () => {
  const g = grouped(FACETED, {
    tagGroups: [{ key: "type", label: "Kind" }],
  })[0];
  return all([
    check(g?.key, toBe("type")),
    check(g?.label, toBe("Kind")),
  ]);
});

test("an undeclared group labels itself with its key", () =>
  check(
    grouped(FACETED, {})[0]?.label,
    toBe(grouped(FACETED, {})[0]?.key),
  ));

// A declared taxonomy fixes the ORDER — not the count order discovery uses.
test("declared variations fix the order", () =>
  check(
    grouped(FACETED, {
      tagGroups: [
        {
          key: "type",
          variations: ["enhancement", "bugfix"],
        },
      ],
    })[0]?.values,
    toEqual(["enhancement", "bugfix"]),
  ));

// An empty shelf is information: a taxonomy says this variation EXISTS, and a
// reader learning that nothing is filed under it has learned something.
test("a declared variation with no documents still shows, counted zero", () => {
  const g = grouped(FACETED, {
    tagGroups: [
      {
        key: "type",
        variations: [
          "bugfix",
          "enhancement",
          "refactor",
        ],
      },
    ],
  })[0];
  return all([
    check(
      g?.values,
      toEqual([
        "bugfix",
        "enhancement",
        "refactor",
      ]),
    ),
    check(g?.counts["refactor"], toBe(undefined)),
  ]);
});

// A document that exists is not made to disappear by a config that forgot it.
test("a variation the corpus carries but the taxonomy omits is appended, not dropped", () =>
  check(
    grouped(FACETED, {
      tagGroups: [
        { key: "type", variations: ["bugfix"] },
      ],
    })[0]?.values,
    toEqual(["bugfix", "enhancement"]),
  ));

// `hide` REPLACES the built-in list, so a corpus whose `title` really is a tag
// can say so.
test("hide replaces the built-in non-dimensions rather than adding to them", () =>
  all([
    // author is hidden, and `type`/`layer` survive
    check(
      grouped(FACETED, { hide: ["author"] }).map(
        (g) => g.key,
      ),
      toEqual(["layer", "type"]),
    ),
    // and with hide declared, a built-in like created_at is NO LONGER hidden
    check(
      grouped(
        {
          "a.md": fm(
            "created_at: 2026-07-15",
            "type: x",
          ),
        },
        { hide: ["author"] },
      ).map((g) => g.key),
      toEqual(["created_at", "type"]),
    ),
  ]));

// Naming a key in both is a contradiction; the one that says "show this" is
// the one written with intent.
test("a declared group outranks the hide list", () =>
  check(
    grouped(FACETED, {
      tagGroups: [{ key: "author" }],
      hide: ["author"],
    }).map((g) => g.key),
    toEqual(["author", "layer", "type"]),
  ));
