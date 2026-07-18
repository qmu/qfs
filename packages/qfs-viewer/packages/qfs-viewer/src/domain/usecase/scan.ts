// The scan: walk the roots, read every document, build the index.
//
// Pure with respect to the filesystem — the `FileSystem` seam is an argument,
// so the whole scan is testable without touching a disk, and the real adapter
// under `vendors/` is the only thing that knows about `node:fs`.
//
// The governing rule is SKIP-AND-COLLECT. Every read is fallible: a file can
// vanish between the walk and the read, a permission can bite, a fence can be
// malformed. None of those may take the index down, because the corpus is
// input, not invariant — one bad file under `packages/` must not stop the
// server from serving the other four hundred. Failures become `ScanError`
// values in the index (workaholic:implementation / functional-programming:
// failure is a return value).
import {
  type SoftStr,
  type Result,
  type Option,
  isOk,
  isSome,
  ok,
  err,
  some,
  none,
  mapOption,
  tryCatch,
} from "plgg";
import {
  parseFrontmatter,
  foldYaml,
} from "plgg-md";
import {
  type FileSystem,
  DEFAULT_ROOTS,
  isPruned,
  isDocumentFile,
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
import {
  type DocumentPath,
  asDocumentPath,
} from "#qfs-viewer/domain/model/Vocabulary";

const errorMessage = (e: unknown): SoftStr =>
  e instanceof Error ? e.message : String(e);

/**
 * Every document path under `root`, depth-first, pruning the directories the
 * scan never descends into.
 *
 * A directory that cannot be listed is skipped, not fatal — the same
 * skip-and-collect rule as a file: an unreadable directory somewhere under
 * `packages/` must not end the walk.
 */
export const walkRoot = (
  fs: FileSystem,
  root: SoftStr,
): ReadonlyArray<SoftStr> => {
  // Each fs touch is wrapped once, here, so the recursion below reads as the
  // pure rule it is: a throw anywhere becomes a skip, never an escaped
  // exception.
  const readDirectorySafely = tryCatch(
    (dir: SoftStr) => fs.readDirectory(dir),
    (): ReadonlyArray<SoftStr> => [],
  );
  const isDirectorySafely = tryCatch(
    (path: SoftStr) => fs.isDirectory(path),
    (): boolean => false,
  );

  const visit = (
    dir: SoftStr,
  ): ReadonlyArray<SoftStr> => {
    const entries = readDirectorySafely(dir);
    // An unreadable directory is skipped, not fatal — the same
    // skip-and-collect rule as a file.
    const names = isOk(entries)
      ? entries.content
      : [];
    return names.flatMap((name) => {
      // The repository root is "."; joining it naively would key every
      // document as "./README.md" — a second spelling of the same document,
      // which the brand exists to prevent.
      const full =
        dir === "." ? name : `${dir}/${name}`;
      const directory = isDirectorySafely(full);
      const isDir =
        isOk(directory) && directory.content;
      return isDir
        ? isPruned(name)
          ? []
          : visit(full)
        : isDocumentFile(name)
          ? [full]
          : [];
    });
  };

  // A root that does not exist is simply empty: not every repository has a
  // `docs/`, and its absence is not an error.
  const present = isDirectorySafely(root);
  return isOk(present) && present.content
    ? visit(root)
    : [];
};

/**
 * What reading one path yielded: the document, plus the front-matter failure
 * if its fence was declined.
 *
 * The two are NOT alternatives, which is why this is a record rather than a
 * `Result`. A document whose front matter plgg-md refuses is still a perfectly
 * readable document — the bytes are fine, the body renders, only the faceting
 * head is unavailable. Modelling it as an error would drop it from the corpus;
 * modelling it as a clean read would hide a real gap. It is both, so it
 * carries both.
 */
export type ReadDocument = Readonly<{
  document: Document;
  /**
   * Set when the file was read but its front matter was not parsed. The
   * document is still indexed, with `frontMatter: None`.
   */
  frontMatterError: Option<ScanError>;
}>;

/**
 * Read one document and parse its front matter. An unreadable file, or a path
 * that is not a valid {@link DocumentPath}, is a collected error rather than a
 * throw.
 *
 * A DECLINED fence is deliberately not a read failure. Ticket …004235's
 * acceptance criteria say a malformed fence is "skipped and reported", written
 * with a genuinely broken document in mind. The failure we actually saw was
 * different in kind: `layer: [Config]` is valid YAML that plgg-md 0.0.2's
 * subset merely did not accept — 7 of this repo's 28 files, 472 of plgg's 661,
 * i.e. the entire ticket corpus. Dropping a document because its `layer:` line
 * used flow style would have emptied the corpus of exactly the tickets a
 * knowledge browser exists to show.
 *
 * That call proved right twice over. It kept the corpus whole while the gap
 * was open, and when 0.0.3 fixed it upstream the documents healed on their own
 * with no change here — which is what "visible, not fatal" buys. The rule
 * still holds for what stays declined (`&`, `!!`, `|`, `>`): index the
 * document with `None` front matter, collect the error beside it.
 */
export const readDocument = (
  fs: FileSystem,
  path: SoftStr,
): Result<ReadDocument, ScanError> => {
  const validPath: Result<DocumentPath, unknown> =
    asDocumentPath(path);
  if (!isOk(validPath)) {
    return err(
      scanError(
        path,
        "not a valid document path (must be relative, non-traversing, and .md)",
      ),
    );
  }
  const source = tryCatch(
    (p: SoftStr) => fs.readFile(p),
    (e: unknown): ScanError =>
      scanError(
        path,
        `could not be read: ${errorMessage(e)}`,
      ),
  )(path);
  if (!isOk(source)) {
    return err(source.content);
  }
  // Total by contract: `parseFrontmatter` never throws, so a fence-less file
  // and a declined one both arrive here as values.
  const parsed = parseFrontmatter(source.content);
  return isOk(parsed)
    ? ok({
        document: document(
          validPath.content,
          source.content,
          // Folded HERE, at the read boundary, so the document carries the
          // plain-data shape both front-matter producers share (see
          // domain/model/Document.ts) and no consumer needs plgg-md's
          // YamlMap vocabulary.
          mapOption(foldYaml)(
            parsed.content.frontmatter.data,
          ),
        ),
        frontMatterError: none(),
      })
    : ok({
        document: document(
          validPath.content,
          source.content,
          none(),
        ),
        frontMatterError: some(
          scanError(
            path,
            `front matter not parsed: ${parsed.content.content.message}`,
          ),
        ),
      });
};

/**
 * Scan every root and build the index.
 *
 * Documents that failed to read are collected into the index's `errors`; the
 * scan always succeeds.
 *
 * Two kinds of error land in the same list, deliberately. An unreadable file
 * yields an error and NO document; a document with a declined fence yields
 * both. `errors` answers "what is wrong with this corpus", and a fence the
 * subset refuses belongs in that answer even though the document is served.
 */
export const scan = (
  fs: FileSystem,
  roots: ReadonlyArray<SoftStr> = DEFAULT_ROOTS,
): Index => {
  const paths = roots.flatMap((r) =>
    walkRoot(fs, r),
  );
  const read = paths.map((p) =>
    readDocument(fs, p),
  );
  const readable = read
    .filter((r) => isOk(r))
    .map((r) => r.content);
  const documents = readable.map(
    (r) => r.document,
  );
  const unreadable = read
    .filter((r) => !isOk(r))
    .map((r) => r.content);
  const declined = readable
    .map((r) => r.frontMatterError)
    .filter((e) => isSome(e))
    .map((e) => e.content);
  return buildIndex(documents, [
    ...unreadable,
    ...declined,
  ]);
};
