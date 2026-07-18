// The corpus, read from qfs's markdown collection path.
//
// This is where the in-process indexer retires INTO qfs (mission acceptance
// item 3; docs/adr/0008). The legacy `scan` walks the tree, parses every
// fence, and a watcher keeps the snapshot honest. Here NONE of that
// machinery exists: qfs's `/markdown/<name>/documents` table is the one
// enumeration and the one front-matter interpretation
// (`workaholic:design` / data-sovereignty — collection and interpretation
// live in one place), and freshness comes from reading PER REQUEST, which
// is docs/adr/0003's own rule for everything qfs answers: a live table's
// whole value is being live, and a watcher over a snapshot would be a
// second, driftable copy of a thing qfs already holds.
//
// What is deliberately NOT read from qfs: the document BODIES. The
// collection path serves no body column — it enumerates and interprets, it
// does not transport bytes — so the bytes for rendering are read
// point-wise, by the path the TABLE named, through the same `FileSystem`
// seam the editor writes through. That is a point read of a named file, not
// a walk and not an interpretation: there is no directory enumeration, no
// fence parse, no pruning rule — nothing that could disagree with qfs about
// what the corpus IS. The two-lock rule still holds: a path enters only as
// a validated `DocumentPath`, off a table row instead of a walk.
import {
  type SoftStr,
  type Result,
  type InvalidError,
  isOk,
  ok,
  err,
  tryCatch,
} from "plgg";
import {
  type FileSystem,
  type ResourceRunner,
} from "#qfs-viewer/domain/model/Scan";
import {
  type Document,
  type ScanError,
  document,
  scanError,
} from "#qfs-viewer/domain/model/Document";
import {
  type Index,
  buildIndex,
} from "#qfs-viewer/domain/model/Index";
import { type IndexRef } from "#qfs-viewer/domain/usecase/reload";
import {
  type CollectionDocument,
  type CollectionLink,
  asCollectionDocuments,
  asCollectionLinks,
  documentsPath,
  documentsStatement,
  linksStatement,
} from "#qfs-viewer/domain/model/Collection";
import { documentPathString } from "#qfs-viewer/domain/model/Vocabulary";

const errorMessage = (e: unknown): SoftStr =>
  e instanceof Error ? e.message : String(e);

// One collection document, made whole: the table's row plus the bytes at
// the path it named. An unreadable file is the same skip-and-collect the
// legacy scan applies — the table asserted a document the disk no longer
// backs (an editor's atomic-rename window, a race with a delete), and that
// is a fact about the corpus worth a line, not a crash.
const readBody = (
  fs: FileSystem,
  doc: CollectionDocument,
): Result<Document, ScanError> => {
  const path = documentPathString(doc.path);
  const source = tryCatch(
    (p: SoftStr) => fs.readFile(p),
    (e: unknown): ScanError =>
      scanError(
        path,
        `the documents table names it, but it could not be read: ${errorMessage(e)}`,
      ),
  )(path);
  return isOk(source)
    ? ok(
        document(
          doc.path,
          source.content,
          doc.frontMatter,
        ),
      )
    : err(source.content);
};

/**
 * The corpus as of NOW, read through the collection path.
 *
 * Always an {@link Index} — including when qfs itself cannot answer. A
 * refused statement (no qfs on PATH, no binding for the tree, a parse
 * error) becomes an EMPTY corpus carrying one error that repeats qfs's own
 * words at the table's address, because the person who declared
 * `collection` in the config is the person who reads the corpus column.
 * Truncation is reported the same way: a listing silently missing its tail
 * is the lie, not the truncation.
 */
export const collectionIndex = (
  runner: ResourceRunner,
  fs: FileSystem,
  name: SoftStr,
): Index => {
  const answer = runner.run(
    documentsStatement(name),
  );
  const parsed = isOk(answer)
    ? asCollectionDocuments(answer.content)
    : answer;
  if (!isOk(parsed)) {
    return buildIndex(
      [],
      [
        scanError(
          documentsPath(name),
          parsed.content.content.message,
        ),
      ],
    );
  }
  const read = parsed.content.documents.map(
    (doc) => readBody(fs, doc),
  );
  const documents = read
    .filter((r) => isOk(r))
    .map((r) => r.content);
  const unreadable = read
    .filter((r) => !isOk(r))
    .map((r) => r.content);
  return buildIndex(documents, [
    ...parsed.content.errors,
    ...unreadable,
    ...(parsed.content.truncated
      ? [
          scanError(
            documentsPath(name),
            "qfs truncated the documents table — this corpus listing is incomplete",
          ),
        ]
      : []),
  ]);
};

/**
 * An {@link IndexRef} whose every `current()` is a fresh read of the
 * collection path.
 *
 * The legacy ref is a mutable cell a watcher swaps; this one holds nothing,
 * because there is nothing to keep fresh — each request reads the corpus as
 * qfs answers it right then, and two handlers in one request still each get
 * one consistent value, exactly the property the cell version guaranteed.
 * `swap` is accepted and DISCARDED: the editor's post-save swap is the
 * legacy path's way of catching up, and here the next read is already
 * current — honoring the swap would install a snapshot that starts aging
 * immediately.
 */
export const collectionRef = (
  runner: ResourceRunner,
  fs: FileSystem,
  name: SoftStr,
): IndexRef => ({
  current: () =>
    collectionIndex(runner, fs, name),
  swap: () => {
    // Deliberately nothing — see the doc comment.
  },
});

/**
 * The links WRITTEN IN one document, in table order, read per request.
 *
 * The statement is a constant of the tree name; the per-document narrowing
 * happens here, so no reader-controlled value is ever interpolated into a
 * statement (domain/model/Collection.ts records the rule).
 */
export const documentLinks = (
  runner: ResourceRunner,
  name: SoftStr,
  source: SoftStr,
): Result<
  ReadonlyArray<CollectionLink>,
  InvalidError
> => {
  const answer = runner.run(linksStatement(name));
  const parsed = isOk(answer)
    ? asCollectionLinks(answer.content)
    : answer;
  return isOk(parsed)
    ? ok(
        parsed.content.filter(
          (link) => link.sourceDoc === source,
        ),
      )
    : err(parsed.content);
};
