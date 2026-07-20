// A filesystem built from a plain map of path -> contents.
//
// Test infrastructure, so it sits under `testkit/` (excluded from the
// production vendor boundary). The scan's rules are pure over the
// `FileSystem` seam, so almost every scan test can state its whole world as a
// literal — no temp dirs, no cleanup, no ordering between tests
// (workaholic:implementation / test: no shared dataset, because it makes
// tests order-dependent and destroys reproducibility).
//
// The real `node:fs` adapter is exercised separately, against a real tree, by
// scan.node.spec.ts — the seam is for speed and determinism, not for avoiding
// the real thing.
import { type SoftStr } from "plgg";
import { type FileSystem } from "#qfs-viewer/domain/model/Scan";

/**
 * A {@link FileSystem} over a literal `path -> contents` map.
 *
 * Directories are implied by the paths: `{"docs/a.md": "x"}` means `docs` is a
 * directory. A path listed in `unreadable` throws on read, which is how a test
 * states "this file exists but bites" — the mid-scan-vanish and
 * permission-denied cases.
 *
 * `hostile` throws on *stat*, and `unlistable` throws on *listing*. They are
 * separate because the walk fails over them at different points: a path that
 * cannot be statted never reaches the listing call, so one set could not
 * exercise both paths.
 */
export const fakeFileSystem = (
  files: Readonly<Record<string, string>>,
  unreadable: ReadonlySet<string> = new Set(),
  hostile: ReadonlySet<string> = new Set(),
  unlistable: ReadonlySet<string> = new Set(),
): FileSystem => {
  const paths = Object.keys(files);

  const directories = new Set<string>();
  for (const p of paths) {
    const segments = p.split("/");
    for (let i = 1; i < segments.length; i += 1) {
      directories.add(
        segments.slice(0, i).join("/"),
      );
    }
  }

  return {
    readDirectory: (dir: SoftStr) => {
      if (
        hostile.has(dir) ||
        unlistable.has(dir)
      ) {
        throw new Error(
          `EACCES: permission denied, scandir '${dir}'`,
        );
      }
      // "." is the tree root — the default scan root. It lists everything
      // top-level, so it takes no prefix.
      const prefix =
        dir === "" || dir === "."
          ? ""
          : `${dir}/`;
      const names = new Set<string>();
      for (const p of [
        ...paths,
        ...directories,
      ]) {
        if (p.startsWith(prefix) && p !== dir) {
          const rest = p.slice(prefix.length);
          const head = rest.split("/")[0];
          if (head !== undefined && head !== "") {
            names.add(head);
          }
        }
      }
      return [...names].sort();
    },
    isDirectory: (path: SoftStr) => {
      if (hostile.has(path)) {
        throw new Error(
          `EACCES: permission denied, stat '${path}'`,
        );
      }
      // The tree root always exists as a directory, even when the corpus is
      // empty — otherwise a scan of an empty tree would report the root as
      // missing rather than as empty.
      return (
        path === "." ||
        path === "" ||
        directories.has(path)
      );
    },
    readFile: (path: SoftStr) => {
      if (unreadable.has(path)) {
        throw new Error(
          `ENOENT: no such file or directory, open '${path}'`,
        );
      }
      const content = files[path];
      if (content === undefined) {
        throw new Error(
          `ENOENT: no such file or directory, open '${path}'`,
        );
      }
      return content;
    },
  };
};
