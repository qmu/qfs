// The watcher against a REAL filesystem.
//
// The reload SEMANTICS are tested without a disk (domain/usecase/reload.spec.ts,
// fake clock, deterministic). What cannot be faked is whether `fs.watch`
// behaves the way this adapter assumes — so these tests drive real files and
// wait for real events. They are the slowest tests in the suite by design:
// every assumption here is about the vendor, and a fake would just agree with
// me (workaholic:implementation / test — test against the real thing).
import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  nodeTimer,
  watchRoots,
} from "#qfs-viewer/vendors/nodeWatcher";
import { type FileChange } from "#qfs-viewer/domain/usecase/reload";

// Poll until `predicate` holds or the budget runs out. A fixed sleep would
// either be flaky or slow; this is as fast as the event allows and fails
// honestly if it never arrives.
const waitFor = async (
  predicate: () => boolean,
  budgetMs: number = 2000,
): Promise<boolean> => {
  const deadline = Date.now() + budgetMs;
  while (Date.now() < deadline) {
    if (predicate()) {
      return true;
    }
    await new Promise((r) => setTimeout(r, 10));
  }
  return predicate();
};

const withTempRoot = async (
  body: (
    cwd: string,
    events: Array<[string, FileChange]>,
    stop: () => void,
  ) => Promise<void>,
): Promise<void> => {
  const cwd = mkdtempSync(
    join(tmpdir(), "qfs-viewer-watch-"),
  );
  mkdirSync(join(cwd, "docs"), {
    recursive: true,
  });
  const events: Array<[string, FileChange]> = [];
  const stop = watchRoots(
    cwd,
    ["docs"],
    (path, change) => {
      events.push([path, change]);
    },
  );
  try {
    await body(cwd, events, stop);
  } finally {
    stop();
    rmSync(cwd, {
      recursive: true,
      force: true,
    });
  }
};

test("nodeTimer schedules, and cancel really cancels", async () => {
  const timer = nodeTimer();
  let fired = 0;
  timer.schedule(() => {
    fired += 1;
  }, 5);
  timer.cancel();
  // The debouncer cancels-then-reschedules on every event, so a cancel that
  // did not actually cancel would fire a reload per keystroke.
  await new Promise((r) => setTimeout(r, 40));
  const afterCancel = fired;

  timer.schedule(() => {
    fired += 1;
  }, 5);
  await new Promise((r) => setTimeout(r, 40));

  return all([
    check(afterCancel, toBe(0)),
    check(fired, toBe(1)),
  ]);
});

test("the real watcher reports a created file as changed", async () => {
  await withTempRoot(async (cwd, events) => {
    writeFileSync(
      join(cwd, "docs", "new.md"),
      "# new",
      "utf8",
    );
    const saw = await waitFor(() =>
      events.some(
        ([p, c]) =>
          p === "docs/new.md" && c === "changed",
      ),
    );
    if (!saw) {
      throw new Error(
        `no changed event for docs/new.md; saw ${JSON.stringify(events)}`,
      );
    }
  });
  return check(true, toBe(true));
});

test("the real watcher reports a deleted file as removed", async () => {
  // The decision this pins: fs.watch reports "rename" for create AND delete,
  // so the adapter asks the filesystem whether the path still exists rather
  // than trusting the event name.
  await withTempRoot(async (cwd, events) => {
    const file = join(cwd, "docs", "doomed.md");
    writeFileSync(file, "# doomed", "utf8");
    await waitFor(() => events.length > 0);
    events.length = 0;
    rmSync(file);
    const saw = await waitFor(() =>
      events.some(
        ([p, c]) =>
          p === "docs/doomed.md" &&
          c === "removed",
      ),
    );
    if (!saw) {
      throw new Error(
        `no removed event for docs/doomed.md; saw ${JSON.stringify(events)}`,
      );
    }
  });
  return check(true, toBe(true));
});

test("the real watcher forwards non-documents and lets the domain judge them", async () => {
  // Nothing is filtered in the vendor layer: a corpus rule there would be a
  // domain decision in the wrong place. applyChange ignores non-documents.
  await withTempRoot(async (cwd, events) => {
    writeFileSync(
      join(cwd, "docs", "notes.txt"),
      "text",
      "utf8",
    );
    const saw = await waitFor(() =>
      events.some(
        ([p]) => p === "docs/notes.txt",
      ),
    );
    if (!saw) {
      throw new Error(
        `expected the .txt to be forwarded; saw ${JSON.stringify(events)}`,
      );
    }
  });
  return check(true, toBe(true));
});

test("watching '.' reaches root-level files and skips node_modules", async () => {
  // The default root is now the whole tree, so "." must be expanded into its
  // non-pruned children plus a flat watch on the root itself — otherwise
  // README.md gets no watcher, or node_modules gets thousands.
  const cwd = mkdtempSync(
    join(tmpdir(), "qfs-viewer-watch-"),
  );
  mkdirSync(join(cwd, "docs"), {
    recursive: true,
  });
  mkdirSync(join(cwd, "node_modules", "dep"), {
    recursive: true,
  });
  const events: Array<[string, FileChange]> = [];
  const stop = watchRoots(cwd, ["."], (p, c) => {
    events.push([p, c]);
  });
  try {
    writeFileSync(
      join(cwd, "README.md"),
      "# readme",
      "utf8",
    );
    const sawRoot = await waitFor(() =>
      events.some(
        ([p, c]) =>
          p === "README.md" && c === "changed",
      ),
    );
    if (!sawRoot) {
      throw new Error(
        `root-level README.md was not watched; saw ${JSON.stringify(events)}`,
      );
    }
    // A nested document under a non-pruned child is still covered.
    writeFileSync(
      join(cwd, "docs", "a.md"),
      "# a",
      "utf8",
    );
    const sawNested = await waitFor(() =>
      events.some(([p]) => p === "docs/a.md"),
    );
    if (!sawNested) {
      throw new Error(
        `docs/a.md was not watched; saw ${JSON.stringify(events)}`,
      );
    }
    // node_modules is excluded from the watch set entirely.
    events.length = 0;
    writeFileSync(
      join(
        cwd,
        "node_modules",
        "dep",
        "README.md",
      ),
      "# theirs",
      "utf8",
    );
    await new Promise((r) => setTimeout(r, 300));
    const leaked = events.filter(([p]) =>
      p.includes("node_modules"),
    );
    if (leaked.length > 0) {
      throw new Error(
        `node_modules should not be watched; saw ${JSON.stringify(leaked)}`,
      );
    }
  } finally {
    stop();
    rmSync(cwd, {
      recursive: true,
      force: true,
    });
  }
  return check(true, toBe(true));
});

test("watching a missing root is skipped, not fatal — not every repo has docs/", () => {
  // fs.watch throws ENOENT on an absent path; that must not take the server
  // down at boot.
  const cwd = mkdtempSync(
    join(tmpdir(), "qfs-viewer-watch-"),
  );
  try {
    const stop = watchRoots(
      cwd,
      ["docs", "packages", ".workaholic"],
      () => {},
    );
    stop();
    return check(true, toBe(true));
  } finally {
    rmSync(cwd, {
      recursive: true,
      force: true,
    });
  }
});
