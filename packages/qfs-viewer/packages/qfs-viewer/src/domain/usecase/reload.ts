// Reload: how the index changes when a file changes.
//
// The whole point of this module is the SWAP. Hot reload is the one part of
// this product with no precedent anywhere in the plgg family — plggpress's dev
// reload works by the bundler re-importing a module with a busted version, and
// its own doc comment says `serve` "loads config ONCE at startup and never
// watches". `npx qfs-viewer` IS serve mode. So the semantics below are
// ours to define, and they are the thing the SSR, REST, and MCP surfaces will
// all quietly depend on.
//
// The semantics, stated once:
//
//   1. A reload produces a NEW index value. It never mutates the old one.
//   2. A reader holds an index VALUE, not a reference to a mutable box. So a
//      read that spans a reload sees the index as of when it started —
//      consistent, never torn, never half-updated.
//   3. The holder (`IndexRef`) is the only mutable cell, and it holds exactly
//      one immutable value. Swapping it is a single assignment: there is no
//      window in which it points at a partially-built index.
//
// (2) is what makes this safe by construction rather than by discipline. A
// design where the reader walked a mutable map would need locking to get the
// same property, and would get it wrong under `await`.
import { type SoftStr, isOk, isSome } from "plgg";
import {
  type FileSystem,
  isPrunedPath,
} from "#qfs-viewer/domain/model/Scan";
import {
  type Index,
  withDocument,
  withoutDocument,
} from "#qfs-viewer/domain/model/Index";
import {
  type DocumentPath,
  asDocumentPath,
} from "#qfs-viewer/domain/model/Vocabulary";
import { readDocument } from "#qfs-viewer/domain/usecase/scan";

/**
 * What happened to a file. A closed set, so every interpreter's `match` is
 * exhaustive and a new kind of change cannot be added without every call site
 * acknowledging it.
 */
export type FileChange = "changed" | "removed";

/**
 * Fold one file change into the index, returning a NEW index.
 *
 * `changed` covers both create and modify: the distinction is not observable
 * from a watch event on most platforms, and it does not matter — re-read the
 * file and set its entry either way. A `changed` file that cannot be read is
 * treated as `removed`, which is the honest reading: an editor's atomic-rename
 * save briefly makes the old path unreadable, and a file that is not there is
 * not in the corpus.
 *
 * Only the changed document is re-read. The rest of the index is carried over
 * by reference — reloading is O(1) in the corpus, not O(n), which is what
 * makes a watch on a large repository viable.
 */
export const applyChange = (
  fs: FileSystem,
  index: Index,
  path: SoftStr,
  change: FileChange,
): Index => {
  // A change inside a pruned directory is not a change to the corpus. This
  // is not the walk — nothing refused to descend here, so the rule must be
  // applied to the finished path, or an `npm install` on a served tree would
  // add every dependency's README one event at a time.
  if (isPrunedPath(path)) {
    return index;
  }
  const validPath = asDocumentPath(path);
  // A change to something that is not a document path (a `.ts` file, a
  // directory, an editor's scratch file) leaves the corpus alone.
  if (!isOk(validPath)) {
    return index;
  }
  const documentPath: DocumentPath =
    validPath.content;
  if (change === "removed") {
    return withoutDocument(index, documentPath);
  }
  const read = readDocument(fs, path);
  // A re-read states this path's errors afresh: a declined fence that was
  // fixed stops reporting, and one that newly breaks starts. An unreadable
  // file is treated as removed — the editor atomic-rename case — which takes
  // its errors with it.
  return isOk(read)
    ? withDocument(
        index,
        read.content.document,
        isSome(read.content.frontMatterError)
          ? [
              read.content.frontMatterError
                .content,
            ]
          : [],
      )
    : withoutDocument(index, documentPath);
};

/**
 * The one mutable cell in the system: a holder for the current index value.
 *
 * Readers call `current()` ONCE and then work with the value they got. That
 * value is immutable, so it cannot change underneath them however long they
 * take. A reload calls `swap()`, which is a single assignment of an
 * already-built value — so there is no instant at which a reader can observe a
 * partially-updated corpus.
 *
 * Deliberately not a `Box`/`Result` type: this is state, and it is the ONLY
 * state. Naming it honestly, and keeping it to four lines, is better than
 * dressing it up as something pure.
 */
export type IndexRef = Readonly<{
  current: () => Index;
  swap: (next: Index) => void;
}>;

/** Builds an {@link IndexRef} holding `initial`. */
export const indexRef = (
  initial: Index,
): IndexRef => {
  let value = initial;
  return {
    current: () => value,
    swap: (next) => {
      value = next;
    },
  };
};

/**
 * A clock, as the debouncer needs it. A seam so reload timing is tested with
 * a fake clock — deterministically, in microseconds — rather than by sleeping
 * and hoping.
 */
export type Timer = Readonly<{
  schedule: (fn: () => void, ms: number) => void;
  cancel: () => void;
}>;

/**
 * Coalesce a burst of file changes into ONE reload.
 *
 * Necessary, not decorative: editors do not emit one event per save. Many
 * write a temp file and rename it (which arrives as several events), and a
 * `git checkout` across a branch can touch hundreds of files in a few
 * milliseconds. Without coalescing, each event would rebuild and swap, and the
 * corpus would visibly churn.
 *
 * Changes are accumulated by path — last change per path wins, since a file
 * created-then-deleted in one window is simply absent — and applied together
 * when the window closes.
 */
export const debouncedReload = (
  fs: FileSystem,
  ref: IndexRef,
  timer: Timer,
  windowMs: number,
): ((
  path: SoftStr,
  change: FileChange,
) => void) => {
  const pending = new Map<SoftStr, FileChange>();
  const flush = (): void => {
    // Build the next value fully, THEN swap once. Swapping per change would
    // expose intermediate states to readers.
    const before = ref.current();
    const next = [...pending.entries()].reduce(
      (acc, [path, change]) =>
        applyChange(fs, acc, path, change),
      before,
    );
    pending.clear();
    // A burst that touched no document leaves the corpus alone, and this
    // guard is what makes that observable rather than merely true.
    //
    // Reference inequality is a SOUND test here, not a shortcut: `applyChange`
    // returns its input unchanged for anything that is not a document (a
    // `.ts` file, a directory, an editor's scratch file, a pruned path), and
    // every real change generates a NEW value because the index is immutable.
    // So `next === before` means precisely "nothing happened".
    //
    // Without it, `swap` fires on EVERY watch event, and the swap callback is
    // where the server logs `index.reloaded`. That made the log lie — claiming
    // a reload on a `.ts` save — and, when the log was written into the tree
    // being watched, it fed itself: 115 reloads in four seconds, none of them
    // real. plgg hit the same shape once already (`Stop the dev server
    // reloading on its own build output`), so this is a family bug, not a
    // one-off. Redirecting the server's own log into its corpus is the most
    // natural thing a person could do; it must not spin.
    if (next !== before) {
      ref.swap(next);
    }
  };
  return (path, change) => {
    pending.set(path, change);
    timer.cancel();
    timer.schedule(flush, windowMs);
  };
};
