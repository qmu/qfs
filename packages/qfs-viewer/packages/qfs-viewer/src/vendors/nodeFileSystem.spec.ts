// The scan against a REAL filesystem.
//
// Every other scan test runs on the `fakeFileSystem` seam — fast and
// deterministic, but it proves only that the rules are consistent with my
// model of a filesystem. If that model is wrong, the fake agrees with the bug.
// This spec is the counterweight (workaholic:implementation / test: test
// against the real thing).
//
// Each test builds its OWN tree under a fresh temp dir and removes it, so no
// shared dataset can make one test depend on another's order.
import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { nodeFileSystem } from "#qfs-viewer/vendors/nodeFileSystem";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import {
  documentCount,
  listDocuments,
  indexErrors,
} from "#qfs-viewer/domain/model/Index";
import { documentPathString } from "#qfs-viewer/domain/model/Vocabulary";

// Build a real tree from a literal, run `body` against it, always clean up.
const withTree = <A>(
  files: Readonly<Record<string, string>>,
  body: (root: string) => A,
): A => {
  const root = mkdtempSync(
    join(tmpdir(), "qfs-viewer-scan-"),
  );
  try {
    for (const [path, content] of Object.entries(
      files,
    )) {
      const full = join(root, path);
      mkdirSync(dirname(full), {
        recursive: true,
      });
      writeFileSync(full, content, "utf8");
    }
    return body(root);
  } finally {
    rmSync(root, {
      recursive: true,
      force: true,
    });
  }
};

test("the real filesystem: scan indexes markdown across the three roots", () =>
  withTree(
    {
      ".workaholic/missions/index.md":
        "# missions",
      "docs/adr/0001.md": "# adr",
      "packages/qfs-viewer/README.md": "# pkg",
      "packages/qfs-viewer/src/main.ts":
        "export const x = 1;",
    },
    (root) => {
      const index = scan(nodeFileSystem(root));
      return all([
        check(documentCount(index), toBe(3)),
        check(
          listDocuments(index).map((d) =>
            documentPathString(d.content.path),
          ),
          toEqual([
            ".workaholic/missions/index.md",
            "docs/adr/0001.md",
            "packages/qfs-viewer/README.md",
          ]),
        ),
        check(indexErrors(index), toEqual([])),
      ]);
    },
  ));

test("the real filesystem: node_modules under packages/ is pruned", () =>
  // The prune that actually matters, proven against a real directory rather
  // than against my idea of one.
  withTree(
    {
      "packages/qfs-viewer/README.md": "# ours",
      "packages/qfs-viewer/node_modules/plgg/README.md":
        "# theirs",
      "packages/qfs-viewer/node_modules/plgg/docs/guide.md":
        "# theirs too",
      "packages/qfs-viewer/dist/out.md":
        "# built",
    },
    (root) => {
      const index = scan(nodeFileSystem(root));
      return all([
        check(documentCount(index), toBe(1)),
        check(
          listDocuments(index).map((d) =>
            documentPathString(d.content.path),
          ),
          toEqual([
            "packages/qfs-viewer/README.md",
          ]),
        ),
      ]);
    },
  ));

test("the real filesystem: a missing root is empty, not an error", () =>
  withTree({ "docs/a.md": "# a" }, (root) => {
    // No .workaholic/ and no packages/ exist in this tree.
    const index = scan(nodeFileSystem(root));
    return all([
      check(documentCount(index), toBe(1)),
      check(indexErrors(index), toEqual([])),
    ]);
  }));

test("the real filesystem: a document's real bytes reach the index", () =>
  withTree(
    { "docs/a.md": "# title\n\nbody text\n" },
    (root) => {
      const index = scan(nodeFileSystem(root));
      const doc = listDocuments(index)[0];
      return check(
        doc?.content.source,
        toBe("# title\n\nbody text\n"),
      );
    },
  ));

test("the real filesystem: an empty corpus scans to an empty index", () =>
  withTree(
    { "docs/notes.txt": "not markdown" },
    (root) =>
      check(
        documentCount(scan(nodeFileSystem(root))),
        toBe(0),
      ),
  ));
