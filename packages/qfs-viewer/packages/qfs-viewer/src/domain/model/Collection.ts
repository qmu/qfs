// The qfs markdown collection path, as this viewer reads it.
//
// qfs's mission `markdown-trees-are-queryable-as-documents-and-links-tables`
// resolves one declared root `<name>` (a `CONNECT /markdown/<name> TO
// markdown AT '<root>'` binding, held in qfs's own ledger) as exactly two
// relational tables:
//
//   /markdown/<name>/documents   path, title, frontmatter (Json | NULL)
//   /markdown/<name>/links       source_doc, source_section_path
//                                (Array(Text)), target, target_doc (| NULL),
//                                line
//
// The path spelling is verified against qfs main, not assumed: qfs #7
// canonicalized the HOSTS realm (`/hosts/<host>/claude`, bare `/claude`
// retired), but `/markdown` is not host-realm-only — the bare
// `/markdown/<name>/…` IS the canonical address qfs's own generated
// docs/drivers.md teaches, so it is what this module speaks.
//
// This module owns what the tables MEAN: the statements the viewer issues
// (and nothing else — the name is validated so nothing outside a closed
// charset can enter a statement), and the reading of qfs's answer into typed
// values. What qfs SAID is `domain/model/Resource.ts`'s `asResourceTable` —
// the same envelope every qfs answer arrives in — so the collection path
// travels the exact seam the declared resources and the generic browsing
// already travel (`workaholic:implementation` /
// anti-corruption-structure: no markdown-specific transport).
import {
  type SoftStr,
  type Option,
  type Result,
  type InvalidError,
  invalidError,
  isOk,
  ok,
  err,
  some,
  none,
} from "plgg";
import {
  type ResourceRow,
  asResourceTable,
} from "#qfs-viewer/domain/model/Resource";
import {
  type ScanError,
  type FrontMatter,
  scanError,
} from "#qfs-viewer/domain/model/Document";
import {
  type DocumentPath,
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

// A collection name is embedded verbatim into `/markdown/<name>/documents`,
// which is embedded verbatim into a statement — so the charset is the
// injection boundary, exactly as `asQfsPath`'s is: no whitespace, no quotes,
// no pipes, no slashes. It matches one path SEGMENT, which is what a
// declared tree name is.
const COLLECTION_NAME = /^[A-Za-z0-9_.-]+$/;

/**
 * Validate the config's `collection` value into a tree name this viewer
 * will address. The error names the rule, because the person who wrote the
 * config reads it.
 */
export const asCollectionName = (
  value: unknown,
): Result<SoftStr, InvalidError> =>
  typeof value === "string" &&
  COLLECTION_NAME.test(value)
    ? ok(value)
    : err(
        invalidError({
          message: `collection must name a qfs markdown tree (one path segment: letters, digits, and ._- only), got ${JSON.stringify(value)}`,
        }),
      );

/**
 * How many rows a collection read asks for.
 *
 * Stated in the statement rather than assumed, so qfs's own `truncated`
 * flag is the one honest signal that a corpus outgrew it — the same rule
 * the generic qfs column follows. Two orders of magnitude above the largest
 * corpus this family serves today (plgg's 1711 documents).
 */
export const COLLECTION_ROW_LIMIT = 100000;

/** The `documents` table's address for a declared tree. */
export const documentsPath = (
  name: SoftStr,
): SoftStr => `/markdown/${name}/documents`;

/** The `links` table's address for a declared tree. */
export const linksPath = (
  name: SoftStr,
): SoftStr => `/markdown/${name}/links`;

/** The one statement that reads the corpus listing. */
export const documentsStatement = (
  name: SoftStr,
): SoftStr =>
  `${documentsPath(name)} |> limit ${COLLECTION_ROW_LIMIT}`;

/** The one statement that reads the link table. */
export const linksStatement = (
  name: SoftStr,
): SoftStr =>
  `${linksPath(name)} |> limit ${COLLECTION_ROW_LIMIT}`;

/**
 * One row of `/markdown/<name>/documents`, typed.
 *
 * No `title`: the strip's document identity is its path (Query.ts records
 * why there is no title anywhere in this viewer), so reading the column
 * would be exported-but-uncallable API — the thing this package has already
 * deleted one round of.
 */
export type CollectionDocument = Readonly<{
  path: DocumentPath;
  /** The `frontmatter` Json column; `None` for a fence-less file. */
  frontMatter: Option<FrontMatter>;
}>;

/** What reading the documents table yielded, skip-and-collect. */
export type CollectionDocuments = Readonly<{
  documents: ReadonlyArray<CollectionDocument>;
  /** Rows the table asserted but this viewer could not accept. */
  errors: ReadonlyArray<ScanError>;
  /** True when qfs stopped early — the corpus listing is incomplete. */
  truncated: boolean;
}>;

const isRecord = (
  v: unknown,
): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

// One documents row. The row is another program's output and therefore a
// boundary: a path that is not a DocumentPath (absolute, traversing, not
// .md) is refused HERE, before it can address a filesystem — the same two
// locks every other entry into the corpus passes.
const readDocumentRow = (
  row: ResourceRow,
): Result<CollectionDocument, ScanError> => {
  const rawPath: unknown = row["path"];
  const path = asDocumentPath(
    typeof rawPath === "string" ? rawPath : "",
  );
  if (!isOk(path)) {
    return err(
      scanError(
        typeof rawPath === "string"
          ? rawPath
          : String(rawPath),
        "the documents table names a path this viewer cannot accept (must be relative, non-traversing, and .md)",
      ),
    );
  }
  const rawFront: unknown = row["frontmatter"];
  // NULL is the honest "no front matter". Anything else must be the JSON
  // OBJECT the column promises — a scalar or array here is qfs breaking its
  // own schema, and saying so beats faceting on garbage.
  if (
    rawFront !== null &&
    rawFront !== undefined &&
    !isRecord(rawFront)
  ) {
    return err(
      scanError(
        documentPathString(path.content),
        "the documents table's frontmatter column did not hold an object",
      ),
    );
  }
  return ok({
    path: path.content,
    frontMatter:
      rawFront === null || rawFront === undefined
        ? none()
        : some(rawFront),
  });
};

/**
 * Read qfs's answer to {@link documentsStatement} into the corpus listing.
 *
 * SKIP-AND-COLLECT over rows, exactly as the legacy scan reads files: one
 * unacceptable row must not take the corpus down, and must not vanish
 * either — it lands in `errors`, which the surfaces show. A malformed
 * ANSWER (no table at all, or qfs's own error object) is the caller's
 * failure to report, so that stays a `Result`.
 */
export const asCollectionDocuments = (
  value: unknown,
): Result<CollectionDocuments, InvalidError> => {
  const table = asResourceTable(value);
  if (!isOk(table)) {
    return err(table.content);
  }
  const read = table.content.rows.map((row) =>
    readDocumentRow(row),
  );
  return ok({
    documents: read
      .filter((r) => isOk(r))
      .map((r) => r.content),
    errors: read
      .filter((r) => !isOk(r))
      .map((r) => r.content),
    truncated: table.content.truncated,
  });
};

/**
 * One row of `/markdown/<name>/links`, typed: one inline markdown link,
 * with the section context the qfs driver preserved.
 */
export type CollectionLink = Readonly<{
  /** The linking document — `documents.path` of where the link was written. */
  sourceDoc: SoftStr;
  /**
   * The full nested heading path of the section containing the link,
   * top-level first; empty for a pre-heading link. Lossless from the
   * driver's Array(Text) — this viewer renders it, it does not type it
   * (relation typing is a later, separate qfs mission).
   */
  sectionPath: ReadonlyArray<SoftStr>;
  /** The target exactly as the author wrote it. */
  target: SoftStr;
  /**
   * The normalized root-relative form, joinable against `documents.path`;
   * `None` for external or root-escaping targets.
   */
  targetDoc: Option<SoftStr>;
  /** 1-based line of the link in its source document. */
  line: number;
}>;

const isStringArray = (
  v: unknown,
): v is ReadonlyArray<string> =>
  Array.isArray(v) &&
  v.every((item) => typeof item === "string");

// One links row, or undefined for a shape the schema does not promise.
// Skip-and-continue rather than collect: a link is a decoration on a
// document column, and the document itself — with its errors surface — is
// not what a malformed link row is about.
const readLinkRow = (
  row: ResourceRow,
): CollectionLink | undefined => {
  const sourceDoc: unknown = row["source_doc"];
  const sectionPath: unknown =
    row["source_section_path"];
  const target: unknown = row["target"];
  const targetDoc: unknown = row["target_doc"];
  const line: unknown = row["line"];
  return typeof sourceDoc === "string" &&
    isStringArray(sectionPath) &&
    typeof target === "string" &&
    (targetDoc === null ||
      targetDoc === undefined ||
      typeof targetDoc === "string") &&
    typeof line === "number"
    ? {
        sourceDoc,
        sectionPath,
        target,
        targetDoc:
          targetDoc === null ||
          targetDoc === undefined
            ? none()
            : some(targetDoc),
        line,
      }
    : undefined;
};

/**
 * Read qfs's answer to {@link linksStatement} into typed links.
 *
 * The rows arrive for the WHOLE tree; the caller filters by `sourceDoc`.
 * Fetching the table and filtering here keeps every statement this viewer
 * issues a CONSTANT of the tree name — no reader-controlled value is ever
 * interpolated into a statement, so there is no quoting rule to get wrong.
 */
export const asCollectionLinks = (
  value: unknown,
): Result<
  ReadonlyArray<CollectionLink>,
  InvalidError
> => {
  const table = asResourceTable(value);
  return isOk(table)
    ? ok(
        table.content.rows
          .map((row) => readLinkRow(row))
          .filter(
            (link): link is CollectionLink =>
              link !== undefined,
          ),
      )
    : err(table.content);
};
