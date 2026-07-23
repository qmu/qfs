// The real filesystem, behind the domain's `FileSystem` seam.
//
// This is the anti-corruption boundary: `node:fs` appears here and nowhere
// under `domain/` (enforced by scripts/gate-vendor-boundary.sh). The adapter
// is deliberately dumb — it translates, it does not decide. Every rule about
// what to walk, what to prune, and what counts as a document lives in
// `domain/model/Scan.ts`, so those rules can be tested without a disk and this
// file has nothing in it worth testing except the translation.
import {
  readdirSync,
  readFileSync,
  statSync,
} from "node:fs";
import { type SoftStr } from "plgg";
import { type FileSystem } from "#qfs-viewer/domain/model/Scan";

/**
 * A {@link FileSystem} over `node:fs`, rooted at `cwd`.
 *
 * Paths crossing the seam are RELATIVE to `cwd` — that is what makes a
 * document's identity (`DocumentPath`) portable and what keeps an absolute
 * machine path out of the index. Resolution to an absolute path happens here
 * and is never visible inward.
 *
 * These calls are synchronous. At scan scale that is the right trade: the walk
 * is a bounded, one-shot startup cost over a repository's own tree, and a
 * synchronous read cannot interleave with a reload — which removes a whole
 * class of torn-read bug rather than managing it. Revisit if a corpus ever
 * grows large enough for the boot walk to be felt.
 */
export const nodeFileSystem = (
  cwd: SoftStr,
): FileSystem => {
  const resolve = (path: SoftStr): SoftStr =>
    `${cwd}/${path}`;
  return {
    readDirectory: (dir) =>
      readdirSync(resolve(dir)),
    isDirectory: (path) =>
      statSync(resolve(path)).isDirectory(),
    readFile: (path) =>
      readFileSync(resolve(path), "utf8"),
  };
};
