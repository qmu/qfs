// The real file watcher, behind the domain's seams.
//
// The anti-corruption boundary for `node:fs.watch`. Everything about WHAT a
// change means to the corpus — which paths matter, how a burst coalesces,
// what a reload produces — lives in domain/usecase/reload.ts and is tested
// without a filesystem. This file only translates events.
//
// There is no upstream precedent to copy: nothing in the plgg family watches
// anything (plggpress's dev reload is the bundler re-importing a module, and
// its `serve` mode "loads config ONCE at startup and never watches"). So the
// translation decisions below are ours, and each one is a judgement about how
// `fs.watch` actually behaves rather than how it is documented.
import {
  watch,
  existsSync,
  readdirSync,
  statSync,
} from "node:fs";
import { type SoftStr } from "plgg";
import {
  type Timer,
  type FileChange,
} from "#qfs-viewer/domain/usecase/reload";
import { isPruned } from "#qfs-viewer/domain/model/Scan";

/**
 * A {@link Timer} over `setTimeout`.
 *
 * `unref()` so a pending debounce cannot hold the process open: a reload
 * scheduled at the moment of shutdown should not delay it.
 */
export const nodeTimer = (): Timer => {
  let handle:
    ReturnType<typeof setTimeout> | undefined =
    undefined;
  return {
    schedule: (fn, ms) => {
      handle = setTimeout(fn, ms);
      handle.unref();
    },
    cancel: () => {
      if (handle !== undefined) {
        clearTimeout(handle);
        handle = undefined;
      }
    },
  };
};

/**
 * Watch `roots` under `cwd`, reporting each changed path (repo-relative) and
 * what happened to it. Returns a function that stops watching.
 *
 * Three translation decisions, each earned rather than assumed:
 *
 * 1. **`existsSync` decides changed-vs-removed, not the event name.** `fs.watch`
 *    reports `rename` for create, delete, AND the temp-file-swap most editors
 *    perform on save — the name says nothing useful. Asking the filesystem
 *    whether the path is there now is the only reliable reading.
 * 2. **A missing root is skipped, not fatal.** Not every repository has a
 *    `docs/`; `fs.watch` throws ENOENT on an absent path, and that must not
 *    take the server down at boot.
 * 3. **Nothing is filtered here.** A `.ts` file, a directory, an editor's
 *    scratch file — all are forwarded, and `applyChange` decides they are not
 *    documents. Filtering here would put a corpus rule in the vendor layer,
 *    which is the thing this boundary exists to prevent.
 */
export const watchRoots = (
  cwd: SoftStr,
  roots: ReadonlyArray<SoftStr>,
  onChange: (
    path: SoftStr,
    change: FileChange,
  ) => void,
): (() => void) => {
  // `fs.watch(recursive)` cannot exclude a subtree, and the default root is
  // now the whole tree — so watching "." directly would place inotify watches
  // across every node_modules in the repository. On the plgg monorepo that is
  // ~4,994 directories instead of ~1,202. Expanding "." into its non-pruned
  // top-level children (plus the root itself, for README.md and friends) keeps
  // the watch set the same shape as the walk's.
  //
  // Correctness does not rest on this — `applyChange` prunes the paths it is
  // handed — but placing thousands of watches on directories whose events are
  // guaranteed to be discarded is waste the product would pay for at every
  // repository it is pointed at.
  const expand = (
    root: SoftStr,
  ): ReadonlyArray<SoftStr> => {
    if (root !== ".") {
      return [root];
    }
    const children = readdirSync(cwd)
      .filter((name) => !isPruned(name))
      .filter((name) =>
        statSync(`${cwd}/${name}`).isDirectory(),
      );
    // "." itself is watched non-recursively for root-level files; each
    // non-pruned child is watched recursively.
    return [".", ...children];
  };

  const watchers = roots
    .flatMap(expand)
    .flatMap((root) => {
      const absolute =
        root === "." ? cwd : `${cwd}/${root}`;
      if (!existsSync(absolute)) {
        return [];
      }
      const w = watch(
        absolute,
        // The root is watched flat: its subtrees are covered by their own
        // watchers, and recursing here would re-introduce the node_modules
        // the expansion just excluded.
        { recursive: root !== "." },
        (_event, filename) => {
          if (filename === null) {
            return;
          }
          // Report paths the way the index keys them: repo-relative, so
          // "README.md" and never "./README.md" — a second spelling of one
          // document is exactly what the brand exists to prevent.
          const path =
            root === "."
              ? `${filename}`
              : `${root}/${filename}`;
          onChange(
            path,
            existsSync(`${cwd}/${path}`)
              ? "changed"
              : "removed",
          );
        },
      );
      // A watcher must not keep the process alive on its own.
      w.unref();
      return [w];
    });
  return () => {
    for (const w of watchers) {
      w.close();
    }
  };
};
