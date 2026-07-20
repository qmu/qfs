// Writing a document back to the working tree.
//
// A SEPARATE seam from `FileSystem`, deliberately. The scan, the index, the
// watcher and every query are readers; only the editor writes. Folding
// `writeFile` into `FileSystem` would hand a write capability to every one of
// them — including the walk, which touches every file in the repository. Two
// seams means the type system says which code can change your disk, and the
// answer is one file (`workaholic:design` / `security`: least authority, and
// `anti-corruption-structure`: the boundary states the capability).
import {
  type SoftStr,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
} from "plgg";
import { type DocumentPath } from "#qfs-viewer/domain/model/Vocabulary";

/**
 * The write half of the filesystem, as the editor needs it.
 *
 * One operation. A seam this small is not ceremony: it is the whole of the
 * authority the editor is granted, and it is why nothing else can write.
 */
export type FileWriter = Readonly<{
  /** Replace `path`'s contents with `source`. */
  writeFile: (
    path: DocumentPath,
    source: SoftStr,
  ) => void;
}>;

/**
 * Reject an edit that is not one, BEFORE it reaches the disk.
 *
 * The rules are deliberately few — this is a knowledge base, not a linter, and
 * a browser that refused to save prose it disliked would be worse than no
 * editor. What it refuses:
 *
 * - **Nothing at all.** An empty body is almost always a browser or a network
 *   losing the form, not an author deleting a document on purpose. Deleting is
 *   a different verb and should look like one.
 * - **A lone `\r`.** Some clients normalise line endings on submit; plgg-md
 *   folds CRLF to LF, so this cannot corrupt a render — but it silently
 *   rewrites every line of the file in git, and a diff nobody intended is a
 *   change nobody reviewed.
 *
 * What it does NOT refuse: front matter this repository's plgg-md cannot
 * parse. That is `/api/errors`'s job to report, and refusing to save it would
 * mean the browser could not fix the very documents it flags — the corpus is
 * input, not invariant.
 */
export const validateEdit = (
  source: SoftStr,
): Result<SoftStr, InvalidError> =>
  source.length === 0
    ? err(
        invalidError({
          message:
            "refusing to save an empty document — deleting a document is a different action, not an empty save",
        }),
      )
    : source.includes("\r")
      ? ok(
          source
            .replaceAll("\r\n", "\n")
            .replaceAll("\r", "\n"),
        )
      : ok(source);
