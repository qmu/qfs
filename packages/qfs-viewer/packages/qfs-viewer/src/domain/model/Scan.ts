// The scan's seams and its pure rules.
//
// The walk itself touches the filesystem and therefore lives under
// `vendors/`; what it MEANS to walk — which roots, which directories to skip,
// what counts as a document — is domain, and lives here. Splitting it this way
// is what lets the scan's rules be tested without a filesystem, and the
// filesystem be tested without the rules (workaholic:implementation /
// functional-programming: keep the shell thin and the core thick).
import {
  type SoftStr,
  type Result,
  type InvalidError,
} from "plgg";

/**
 * A filesystem, as the scan needs it. The seam the real `node:fs` adapter
 * (under `vendors/`) implements and the tests substitute.
 *
 * Deliberately tiny: three operations is the whole of what a scan does, and a
 * seam that mirrored `node:fs` would drag the vendor's shape into the domain —
 * the thing the anti-corruption boundary exists to prevent.
 */
export type FileSystem = Readonly<{
  /** Entry names directly under `dir`. */
  readDirectory: (
    dir: SoftStr,
  ) => ReadonlyArray<SoftStr>;
  /** Whether `path` is a directory. */
  isDirectory: (path: SoftStr) => boolean;
  /** The UTF-8 contents of `path`. */
  readFile: (path: SoftStr) => SoftStr;
}>;

/**
 * Running a qfs statement, as the resource surface needs it.
 *
 * Its own seam beside `FileSystem` and `FileWriter`, for the same reason they
 * are separate from each other: this is a different authority. `FileSystem`
 * reads markdown under one root; this one can reach a database, a mailbox, or
 * a cloud account, and the type says which code holds that reach — one file.
 *
 * Returns `unknown`, not a table. What qfs said and what it MEANS are two
 * questions, and the second belongs to `domain/model/Resource.ts` where it can
 * be tested without a qfs.
 */
export type ResourceRunner = Readonly<{
  run: (
    statement: SoftStr,
  ) => Result<unknown, InvalidError>;
  /**
   * `qfs describe <path>` — the first half of qfs's own describe→preview
   * loop, and what the generic default view is lowered from. Pure on the
   * qfs side (no credentials, no writes), so granting `run` and `describe`
   * together grants no authority `run` alone did not.
   */
  describe: (
    path: SoftStr,
  ) => Result<unknown, InvalidError>;
}>;

/**
 * The roots a scan walks, relative to the directory qfs-viewer was run
 * from.
 *
 * **The whole tree**, pruned — not an allowlist of directories.
 *
 * The first cut named `.workaholic`, `docs`, and `packages` explicitly, which
 * read the mission's *examples* (「.workaholic/とdocs/、packages/などにも散らばる」
 * — `など` means "and such") as an exhaustive list. The result was a knowledge
 * base that did not contain its own repository's `README.md`: the four
 * documents it missed were `README.md`, `CLAUDE.md`, and the two under
 * `workloads/`.
 *
 * Inverting it — take everything, subtract the noise — is both the faithful
 * reading and the more durable one: the next directory someone adds is
 * included for free, where an allowlist would silently omit it and nobody
 * would notice until a document was missing.
 *
 * `PRUNED_DIRECTORIES` is what makes this safe rather than reckless.
 */
export const DEFAULT_ROOTS: ReadonlyArray<SoftStr> =
  ["."];

/**
 * Directories a scan never descends into.
 *
 * `node_modules` and `dist` are the load-bearing entries: `packages/` is a
 * scan root, and a single unpruned `node_modules` would pull tens of
 * thousands of foreign markdown files into the corpus — every dependency's
 * README presented as this repository's knowledge. `.git` holds no documents;
 * `outputs` is runtime output (workaholic:implementation /
 * directory-structure) and is gitignored.
 *
 * `.worktrees` is the same failure as `node_modules`, wearing the corpus's own
 * face — and it was found in the wild rather than reasoned about. Pointed at
 * plgg, this tool reported **1714 documents of which 761 lived under
 * `.worktrees/`**: 44% of the corpus was the same repository's knowledge at
 * other commits, listed a second and third time beside itself. The `1714` was
 * quoted as evidence for a whole session before anyone looked at what it
 * counted.
 *
 * It matters more here than the number suggests, because a git worktree is not
 * an exotic layout in this house — it is the METHOD. Every `/trip` and every
 * parallel `/drive` makes one, so a tool for browsing workaholic repositories
 * duplicated exactly the repositories it exists for, and did it worst on the
 * biggest ones.
 *
 * A worktree is a transient checkout of ANOTHER branch: its documents are this
 * corpus's documents at a different commit, so serving them does not add
 * knowledge, it adds copies — and a reader searching a phrase gets the same
 * document back three times with no way to tell which is the live one. The
 * whole-tree-minus-noise rule above is only safe while the noise list is
 * honest about what noise there is.
 */
export const PRUNED_DIRECTORIES: ReadonlySet<string> =
  new Set([
    "node_modules",
    "dist",
    ".git",
    ".worktrees",
    "outputs",
    "coverage",
  ]);

/** Whether a scan descends into a directory of this name. */
export const isPruned = (
  name: SoftStr,
): boolean => PRUNED_DIRECTORIES.has(name);

/**
 * Whether a PATH lies inside a pruned directory, at any depth.
 *
 * The walk prunes by refusing to descend, which is enough while walking. The
 * reload path has no walk: a watch event hands over a finished path, and
 * `node_modules/plgg/README.md` is a perfectly valid `DocumentPath` — so
 * without this, an `npm install` on a served tree would inject every
 * dependency's README into the corpus, one watch event at a time. The rule
 * that "a dependency's README is not this corpus" has to hold on BOTH paths,
 * so it lives here, once, and both call it.
 */
export const isPrunedPath = (
  path: SoftStr,
): boolean =>
  path
    .split("/")
    .some((segment) => isPruned(segment));

/**
 * Whether a filename is a document.
 *
 * Markdown only, and never a dotfile: an editor's `.foo.md.swp`-adjacent
 * scratch file is not corpus. Case-insensitive on the extension because the
 * corpus is other people's files.
 */
export const isDocumentFile = (
  name: SoftStr,
): boolean =>
  !name.startsWith(".") &&
  name.toLowerCase().endsWith(".md");
