// `qfs-viewer mcp` — the corpus, served to an agent over stdio.
//
// The composition root for the MCP surface, and the twin of `serve.ts`: the
// same config-driven corpus source (the `collection` switch, docs/adr/0008),
// the same index model, the same reload semantics on the legacy arm — a
// different mouth. An agent that queries this while a developer edits a
// file sees the edit, because both read one truth.
//
// STDIO IS THE PROTOCOL STREAM, which is why this file logs to stderr and
// `serve.ts` logs to stdout. A single structured line written to stdout here
// would land in the middle of a JSON-RPC frame and break the session — the
// kind of bug that looks like a client incompatibility for an hour. plgg-mcp's
// own doc comment says the same: "stderr is left for logs so it never corrupts
// the protocol stream".
import { isOk } from "plgg";
import { runStdioServer } from "plgg-mcp";
import { nodeFileSystem } from "#qfs-viewer/vendors/nodeFileSystem";
import {
  loadConfig,
  CONFIG_FILE,
} from "#qfs-viewer/vendors/nodeConfigLoader";
import { qfsRunner } from "#qfs-viewer/vendors/qfsRunner";
import {
  nodeTimer,
  watchRoots,
} from "#qfs-viewer/vendors/nodeWatcher";
import { scan } from "#qfs-viewer/domain/usecase/scan";
import {
  type IndexRef,
  indexRef,
  debouncedReload,
} from "#qfs-viewer/domain/usecase/reload";
import { collectionRef } from "#qfs-viewer/domain/usecase/collection";
import { DEFAULT_ROOTS } from "#qfs-viewer/domain/model/Scan";
import {
  documentCount,
  indexErrors,
  newErrors,
} from "#qfs-viewer/domain/model/Index";
import { insightTools } from "#qfs-viewer/entrypoints/mcpTools";

/** The debounce window, matching `serve.ts` for the same reasons. */
const DEBOUNCE_MS = 50;

// To STDERR, never stdout — see the header.
const logEvent = (
  fields: Readonly<Record<string, unknown>>,
): void => {
  process.stderr.write(
    `${JSON.stringify(fields)}\n`,
  );
};

// The legacy corpus source, kept verbatim behind the config's `collection`
// switch — the same switch and the same recorded retirement date
// (2026-07-31) as `serve.ts`'s arm; docs/adr/0008 is the record.
const attachScan = (
  fs: ReturnType<typeof nodeFileSystem>,
  cwd: string,
  startedAt: number,
): IndexRef => {
  logEvent({
    event: "scan.start",
    root: cwd,
    roots: DEFAULT_ROOTS,
    surface: "mcp",
  });

  const index = scan(fs);
  const ref = indexRef(index);

  logEvent({
    event: "scan.complete",
    root: cwd,
    documents: documentCount(index),
    errors: indexErrors(index).length,
    durationMs: Date.now() - startedAt,
    surface: "mcp",
  });
  for (const e of indexErrors(index)) {
    logEvent({
      event: "scan.error",
      path: e.content.path,
      message: e.content.message,
    });
  }

  const reload = debouncedReload(
    fs,
    {
      current: ref.current,
      swap: (next) => {
        const previous = ref.current();
        ref.swap(next);
        logEvent({
          event: "index.reloaded",
          documents: documentCount(next),
          errors: indexErrors(next).length,
        });
        for (const e of newErrors(
          previous,
          next,
        )) {
          logEvent({
            event: "scan.error",
            path: e.content.path,
            message: e.content.message,
          });
        }
      },
    },
    nodeTimer(),
    DEBOUNCE_MS,
  );
  watchRoots(cwd, DEFAULT_ROOTS, reload);
  return ref;
};

// The collection corpus source (docs/adr/0008): the same per-request reads
// the HTTP surface makes, so an agent and a browser pointed at one tree
// read one truth. No scanner and no watcher are constructed on this arm —
// a long-lived agent session sees an edit because every tool call reads
// the collection path afresh, not because anything here watched.
const attachCollection = (
  runner: ReturnType<typeof qfsRunner>,
  fs: ReturnType<typeof nodeFileSystem>,
  name: string,
  startedAt: number,
): IndexRef => {
  const ref = collectionRef(runner, fs, name);
  const probe = ref.current();
  logEvent({
    event: "collection.read",
    tree: name,
    documents: documentCount(probe),
    errors: indexErrors(probe).length,
    durationMs: Date.now() - startedAt,
    surface: "mcp",
  });
  for (const e of indexErrors(probe)) {
    logEvent({
      event: "scan.error",
      path: e.content.path,
      message: e.content.message,
    });
  }
  return ref;
};

/**
 * Serve the corpus at `cwd` over MCP on stdio.
 *
 * The corpus source is the config's `collection` switch, exactly as it is
 * for HTTP (docs/adr/0008): declared, the qfs collection path answers every
 * tool call afresh; absent, the legacy scan-and-watch serves until its
 * retirement date. Either way an agent holding a long session sees a
 * document the developer just edited, without restarting.
 */
export const serveMcp = (cwd: string): void => {
  const startedAt = Date.now();
  const fs = nodeFileSystem(cwd);

  // The same stop-the-boot rule as `serve.ts`, for the same reason: a
  // malformed config is a question its author wants answered, and an agent
  // session quietly started against the wrong corpus source answers it
  // slowest. Logged to stderr and NOT served — a dead transport plus one
  // line saying why beats a live one lying about its corpus.
  const config = loadConfig(cwd);
  if (!isOk(config)) {
    logEvent({
      event: "config.invalid",
      file: CONFIG_FILE,
      message: config.content.content.message,
    });
    return;
  }
  const collection = config.content.collection;
  const ref: IndexRef =
    collection.__tag === "Some"
      ? attachCollection(
          qfsRunner(config.content.qfs),
          fs,
          collection.content,
          startedAt,
        )
      : attachScan(fs, cwd, startedAt);

  logEvent({
    event: "mcp.listening",
    transport: "stdio",
    tools: insightTools(ref).map((t) => t.name),
  });

  runStdioServer(insightTools(ref), {
    name: "qfs-viewer",
    version: "0.0.1",
  });
};
