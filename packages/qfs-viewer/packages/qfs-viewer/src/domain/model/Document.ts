// A document: one markdown file in the corpus.
//
// A document is its identity, its source text, and its **front matter** — the
// tagged head the index facets on. Front matter is `Option`, and the `None`
// case carries real weight: a fence-less file has none (which is normal, not
// an error), and so does a file whose head the producer declined to interpret.
//
// The front matter is FOLDED PLAIN DATA (`Record<string, unknown>`), not
// plgg-md's `YamlMap`, and that shape is load-bearing: front matter now has
// TWO producers, and this is the shape they share. The legacy scan parses a
// fence with plgg-md's deliberately bounded subset and folds it through
// `foldYaml` (plgg-md's own one-truth bridge to plain data); the qfs
// collection path (`/markdown/<name>/documents`, docs/adr/0008) delivers the
// `frontmatter` column as JSON that already IS plain data. Storing `YamlMap`
// here would force the qfs answer through a lossy re-encoding into a subset
// qfs never promised — a second interpretation of a thing the collection
// path exists to interpret once.
//
// What plgg-md's subset declines on the legacy path stays declined: alias
// expansion (`&`), tags (`!!`), merge keys, and block scalars (`|`/`>`) are
// genuine attack surface, and fail-closed is the point. `None` stays the
// honest answer for those, and the document is still served.
import {
  type SoftStr,
  type Option,
  type Box,
  box,
} from "plgg";
import { type DocumentPath } from "#qfs-viewer/domain/model/Vocabulary";

/**
 * A document's folded front matter: plain JSON-ish data, keyed by field.
 *
 * The shared shape of both producers (see the module header), and the exact
 * output type of plgg-md's `foldYaml` — so every consumer (facets, filters,
 * the REST and MCP detail surfaces) reads one vocabulary with no per-producer
 * branch.
 */
export type FrontMatter = Readonly<
  Record<string, unknown>
>;

/** One markdown file in the corpus. */
export type Document = Box<
  "Document",
  Readonly<{
    path: DocumentPath;
    source: SoftStr;
    /**
     * The folded front matter, or `None` — for a fence-less file, or one
     * whose head the producer declined (plgg-md's subset on the legacy
     * scan; a NULL `frontmatter` column on the collection path). A document
     * is readable either way; only faceting needs this.
     */
    frontMatter: Option<FrontMatter>;
  }>
>;

/** Builds a {@link Document}. */
export const document = (
  path: DocumentPath,
  source: SoftStr,
  frontMatter: Option<FrontMatter>,
): Document =>
  box("Document")({
    path,
    source,
    frontMatter,
  });

/**
 * A document that could not be read or parsed during a scan.
 *
 * A scan collects these rather than failing: one malformed file under
 * `packages/` must not take the whole index — and therefore the server —
 * down. The corpus is other people's markdown; it is input, not invariant.
 */
export type ScanError = Box<
  "ScanError",
  Readonly<{
    path: SoftStr;
    message: SoftStr;
  }>
>;

/** Builds a {@link ScanError}. */
export const scanError = (
  path: SoftStr,
  message: SoftStr,
): ScanError =>
  box("ScanError")({ path, message });
