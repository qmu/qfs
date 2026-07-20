// The real filesystem's WRITE half, behind the domain's `FileWriter` seam.
//
// Its own file rather than a fourth method on `nodeFileSystem`, mirroring the
// split in the domain: readers get a reader, and the one thing that writes
// gets a writer. `node:fs` appears here and nowhere under `domain/`
// (scripts/gate-vendor-boundary.sh enforces it).
//
// Dumb by design, like its sibling: it translates, it does not decide. Whether
// an edit is allowed is `domain/model/Editor.ts`'s question, and it is
// answered before anything reaches this file.
import { writeFileSync } from "node:fs";
import { type SoftStr } from "plgg";
import { type FileWriter } from "#qfs-viewer/domain/model/Editor";
import { documentPathString } from "#qfs-viewer/domain/model/Vocabulary";

/**
 * A {@link FileWriter} over `node:fs`, rooted at `cwd`.
 *
 * The path crossing the seam is a `DocumentPath` — branded, so it has already
 * passed the predicate that refuses an absolute or traversing path. That is
 * the lock that matters here: this function resolves against `cwd` and writes,
 * and a bare string could have pointed anywhere on the disk. The brand is why
 * this file can be four lines and still be safe.
 *
 * Synchronous, like the reader, and for a sharper reason: the caller applies
 * the same edit to the index immediately afterwards, and an async write would
 * let the two disagree about whether the document on disk had changed yet.
 */
export const nodeFileWriter = (
  cwd: SoftStr,
): FileWriter => ({
  writeFile: (path, source) => {
    writeFileSync(
      `${cwd}/${documentPathString(path)}`,
      source,
      "utf8",
    );
  },
});
