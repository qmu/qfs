// The index: the on-memory model of the whole corpus.
//
// Not a cache — the serving model itself
// (docs/adr/0003-no-caching.md). The distinction is authority: the index is
// derived from the current corpus and invalidated by the watcher, whereas a
// cache serves a past answer on the bet that it is still true.
//
// The index is an immutable VALUE. A reload does not mutate it; it generates
// a new one (workaholic:implementation / functional-programming: "we treat
// data as immutable and express change as the generation of a new value").
// That is what makes a read that holds an index reference across an `await`
// safe by construction: the value it holds cannot change underneath it, so it
// cannot observe a half-updated corpus. Every function here therefore returns
// a new `Index` and leaves its input untouched — a property the spec asserts
// directly, because it is the one invariant the SSR, REST, and MCP surfaces
// all depend on.
import {
  type Option,
  type SoftStr,
  type Box,
  box,
  some,
  none,
  fromNullable,
} from "plgg";
import {
  type Document,
  type ScanError,
} from "#qfs-viewer/domain/model/Document";
import {
  type DocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";

/**
 * The corpus, keyed by document path, plus the errors the scan collected.
 *
 * `errors` is part of the value, not a side channel: a document that failed to
 * parse is a fact about the corpus that the surfaces should be able to show,
 * not something to swallow into a log line.
 */
export type Index = Box<
  "Index",
  Readonly<{
    documents: ReadonlyMap<string, Document>;
    errors: ReadonlyArray<ScanError>;
  }>
>;

const make = (
  documents: ReadonlyMap<string, Document>,
  errors: ReadonlyArray<ScanError>,
): Index => box("Index")({ documents, errors });

/**
 * Builds an index from a scan's documents and collected errors.
 *
 * A later document with the same path wins — the walk yields each path once,
 * so this only arises if a caller concatenates scans, where "most recent
 * reading of that path" is the sane rule.
 *
 * `errors` is required rather than defaulted: every real caller has them, and
 * a default would let a caller quietly drop the corpus's failures.
 */
export const buildIndex = (
  documents: ReadonlyArray<Document>,
  errors: ReadonlyArray<ScanError>,
): Index =>
  make(
    new Map(
      documents.map((d) => [
        documentPathString(d.content.path),
        d,
      ]),
    ),
    errors,
  );

/** The document at a path, if the corpus holds one. */
export const getDocument = (
  index: Index,
  path: DocumentPath,
): Option<Document> =>
  fromNullable(
    index.content.documents.get(
      documentPathString(path),
    ),
  );

/**
 * Every document, in path order — a stable listing for the surfaces.
 *
 * The comparator has no equal case on purpose: these are Map keys, so two
 * entries cannot share a path, and an `a === b ? 0` arm would be a branch no
 * input could ever reach.
 */
export const listDocuments = (
  index: Index,
): ReadonlyArray<Document> =>
  [...index.content.documents.entries()]
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([, d]) => d);

/** How many documents the corpus holds. */
export const documentCount = (
  index: Index,
): number => index.content.documents.size;

/** The errors the scan collected. */
export const indexErrors = (
  index: Index,
): ReadonlyArray<ScanError> =>
  index.content.errors;

// Identity of an error, for comparing two indexes. Path alone is not enough:
// a file can be fixed in one way and broken in another between two reloads,
// and that is a new fact worth a new line rather than a silent carry-over.
//
// The separator is NUL because it is the one byte that can appear in neither
// a path nor a message, so no pair of fields can collide by containing the
// separator themselves. Write it as the ESCAPE `\0` and never as a literal
// byte: it was a literal here, and a raw NUL made this file `data` to
// `file(1)` and therefore BINARY to grep -- which skips binary content
// silently. So this file answered "no match" to every search, while looking
// perfectly normal on screen (the NUL renders as a space in an editor and in
// a diff). A rename across the package missed exactly these two imports for
// that reason, and the search that "verified" the rename reported zero
// because it could not read the file either. The escape has the identical
// string value and keeps the source greppable.
const errorKey = (e: ScanError): string =>
  `${e.content.path}\0${e.content.message}`;

/**
 * The errors present in `after` that were not in `before`.
 *
 * Exists for the reload log. At boot every collected error is written out one
 * line each, but a reload only reported COUNTS — `index.reloaded {errors: 8}`
 * — so a fence that broke while the server was running named no file and the
 * one line that could have said which was never written. That is the gap this
 * closes, and it is pure so the rule is tested here rather than in the
 * composition root, which no spec loads.
 *
 * Deliberately one-directional: an error that DISAPPEARED is the author
 * fixing something, which the next `index.reloaded` count already shows and
 * which nobody needs woken for.
 */
export const newErrors = (
  before: Index,
  after: Index,
): ReadonlyArray<ScanError> => {
  const had = new Set(
    before.content.errors.map(errorKey),
  );
  return after.content.errors.filter(
    (e) => !had.has(errorKey(e)),
  );
};

// Errors are keyed by the path they concern, so re-reading a path can state
// its errors afresh. Anything a reload does not speak to is left alone.
const errorsExcept = (
  errors: ReadonlyArray<ScanError>,
  path: DocumentPath,
): ReadonlyArray<ScanError> => {
  const key = documentPathString(path);
  return errors.filter(
    (e) => e.content.path !== key,
  );
};

/**
 * A NEW index with `doc` added or replaced, and that path's errors restated.
 * The input index is untouched.
 *
 * This is the reload path for a changed file: re-read one document, swap its
 * entry, leave every other entry identical.
 *
 * `pathErrors` REPLACES whatever was previously recorded against this path
 * rather than adding to it, and it is required rather than defaulted. The
 * index's `errors` describe the corpus as it is now, so a re-read has to be
 * able to clear an error as well as raise one: a file whose fence is fixed
 * must stop reporting, and one that newly breaks must start. A default of
 * `[]` would let a caller silently drop a real failure, the same reason
 * `buildIndex` takes its errors explicitly.
 */
export const withDocument = (
  index: Index,
  doc: Document,
  pathErrors: ReadonlyArray<ScanError>,
): Index => {
  const next = new Map(index.content.documents);
  next.set(
    documentPathString(doc.content.path),
    doc,
  );
  return make(next, [
    ...errorsExcept(
      index.content.errors,
      doc.content.path,
    ),
    ...pathErrors,
  ]);
};

/**
 * A NEW index with the document at `path` removed, along with any errors
 * recorded against it. The input is untouched. Removing an absent path is a
 * no-op that still yields a new value, so callers need no "did it exist"
 * dance.
 *
 * A deleted file's errors go with it: an error about a document that is no
 * longer in the corpus is not a fact about the corpus.
 */
export const withoutDocument = (
  index: Index,
  path: DocumentPath,
): Index => {
  const next = new Map(index.content.documents);
  next.delete(documentPathString(path));
  return make(
    next,
    errorsExcept(index.content.errors, path),
  );
};

/**
 * The source of the document at `path`, if present — the common read the
 * surfaces make, without unwrapping the document themselves.
 */
export const documentSource = (
  index: Index,
  path: DocumentPath,
): Option<SoftStr> => {
  const doc = index.content.documents.get(
    documentPathString(path),
  );
  return doc === undefined
    ? none()
    : some(doc.content.source);
};
