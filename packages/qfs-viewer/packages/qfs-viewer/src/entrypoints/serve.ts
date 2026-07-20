// `qfs-viewer serve` — scan the working tree, then serve it.
//
// The composition root: the one place that wires the real filesystem to the
// domain and the domain to a port. It is an entrypoint, so `node:*` and the
// platform seam are allowed here and nowhere inward.
//
// The index is scanned once at boot, held in an `IndexRef`, and hot-reloaded:
// the watcher reports changes, `debouncedReload` coalesces them and swaps in a
// NEW index value. Readers hold values, so a reload cannot tear a request in
// flight (see domain/usecase/reload.ts for the semantics).
import { pipe, isOk } from "plgg";
import { toFetch } from "plgg-server";
import { serve } from "plgg-server/node";
import { nodeFileSystem } from "#qfs-viewer/vendors/nodeFileSystem";
import { nodeFileWriter } from "#qfs-viewer/vendors/nodeFileWriter";
import {
  loadConfig,
  CONFIG_FILE,
} from "#qfs-viewer/vendors/nodeConfigLoader";
import { type Config } from "#qfs-viewer/domain/model/Config";
import {
  qfsRunner,
  probeQfs,
} from "#qfs-viewer/vendors/qfsRunner";
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
import { documentsPath } from "#qfs-viewer/domain/model/Collection";
import {
  type FileSystem,
  DEFAULT_ROOTS,
  type ResourceRunner,
} from "#qfs-viewer/domain/model/Scan";
import {
  documentCount,
  indexErrors,
  newErrors,
} from "#qfs-viewer/domain/model/Index";
import { api } from "#qfs-viewer/entrypoints/api";

/**
 * How long to wait after a change before reloading.
 *
 * Editors do not emit one event per save, and a `git checkout` can touch
 * hundreds of files in milliseconds. Long enough to coalesce a burst, short
 * enough that a save feels immediate.
 */
const DEBOUNCE_MS = 50;

/**
 * Write one structured event to stdout.
 *
 * One line, one JSON object, newline-delimited — the shape a log shipper
 * expects, and the reason `Indexed 47 files` via `console.log` is the named
 * anti-pattern (`workaholic:implementation` / `observability`). It goes
 * through `stdout` rather than `console` so the stream is explicit and the
 * output is not at the mercy of a `console` shim.
 *
 * Here rather than in a module of its own: this is the only file that logs,
 * because logging is shell work and the domain returns values instead
 * (`workaholic:implementation` / `functional-programming`). If a second
 * entrypoint ever needs it, that is when it earns a home.
 */
const logEvent = (
  fields: Readonly<Record<string, unknown>>,
): void => {
  process.stdout.write(
    `${JSON.stringify(fields)}\n`,
  );
};

/**
 * The legacy corpus source: scan the tree once, watch it, hot-reload.
 *
 * The pre-collection arrangement, kept verbatim behind the config's
 * `collection` switch until its recorded retirement date (2026-07-31,
 * docs/adr/0008). Everything about it — the walk, the fence parsing, the
 * watcher — is what retires; nothing new may come to depend on it.
 */
const attachScan = (
  fs: FileSystem,
  cwd: string,
  config: Config,
  startedAt: number,
): IndexRef => {
  // Emitted BEFORE the walk, so a scan that never finishes still says so. The
  // complete/error lines below can only be written by a scan that returned; a
  // corpus large enough to hang, or a root that turns out to be a symlink
  // loop, would otherwise produce total silence and look like a startup crash.
  logEvent({
    event: "scan.start",
    root: cwd,
    roots: DEFAULT_ROOTS,
    tagGroups: config.tagGroups.map((g) => g.key),
  });

  const index = scan(fs);
  const ref = indexRef(index);

  logEvent({
    event: "scan.complete",
    root: cwd,
    documents: documentCount(index),
    errors: indexErrors(index).length,
    durationMs: Date.now() - startedAt,
  });

  // A corpus's failures are worth one line each at boot: a document that
  // silently never indexed is the bug this makes visible.
  for (const e of indexErrors(index)) {
    logEvent({
      event: "scan.error",
      path: e.content.path,
      message: e.content.message,
    });
  }

  // Hot reload. The watcher only translates fs events; `debouncedReload`
  // decides what they mean to the corpus and produces the new index value.
  const reload = debouncedReload(
    fs,
    {
      current: ref.current,
      swap: (next) => {
        // Read the outgoing value BEFORE swapping: after the assignment the
        // old index is gone and there is nothing left to compare against.
        const previous = ref.current();
        ref.swap(next);
        logEvent({
          event: "index.reloaded",
          documents: documentCount(next),
          errors: indexErrors(next).length,
        });
        // A count alone cannot be acted on. Boot names every bad file one line
        // each; a reload used to name none, so a fence broken while the server
        // ran left `errors: 8` and no way to learn which of the 8 was new.
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

/**
 * The collection corpus source (docs/adr/0008): qfs's
 * `/markdown/<name>/documents` is the enumeration and the interpretation,
 * read per request. NO scanner and NO watcher exist on this arm — they are
 * never constructed, which is what "verifiably inert" means here — and
 * freshness is a property of reading per request rather than of anything
 * this function keeps.
 *
 * The one read at boot is a PROBE, not a snapshot: its value is discarded,
 * and it exists so a qfs that cannot answer (not on PATH, no binding for
 * the tree) says so in the boot log rather than on the first request
 * someone makes tomorrow.
 */
const attachCollection = (
  runner: ResourceRunner,
  fs: FileSystem,
  name: string,
  startedAt: number,
): IndexRef => {
  const ref = collectionRef(runner, fs, name);
  logEvent({
    event: "collection.attached",
    tree: name,
    path: documentsPath(name),
  });
  const probe = ref.current();
  logEvent({
    event: "collection.read",
    tree: name,
    documents: documentCount(probe),
    errors: indexErrors(probe).length,
    durationMs: Date.now() - startedAt,
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
 * Scan `cwd` and serve the corpus on `port`.
 *
 * `readOnly` withholds write authority: no `/edit`, no writer, by construction
 * rather than by a check. Default false, because the mission's local surface is
 * "browsable AND editable in place" — but a server pointed at a directory the
 * runner does not own must not be able to write to it.
 *
 * Logs are structured JSON, not prose: `Indexed 47 files` via console.log is
 * the named anti-pattern (workaholic:implementation / observability, and
 * docs/adr/0006 for why the metrics half is absent).
 */
export const serveCorpus = (
  cwd: string,
  port: number,
  readOnly: boolean = false,
): number => {
  const startedAt = Date.now();
  const fs = nodeFileSystem(cwd);

  // A malformed config STOPS the boot. Starting anyway with the default would
  // discard what someone wrote on purpose and say nothing — they would find
  // out by noticing their facets were wrong, which is a slower and more
  // confusing way to learn about a trailing comma. An ABSENT config is not an
  // error and is the normal case.
  const config = loadConfig(cwd);
  if (!isOk(config)) {
    logEvent({
      event: "config.invalid",
      file: CONFIG_FILE,
      message: config.content.content.message,
    });
    // RETURNED, not assigned to `process.exitCode`. Setting it here did not
    // survive: `cli.ts` does `process.exitCode = main()`, and `main` returned
    // 0 for `serve` unconditionally — so a malformed config printed its error
    // and then exited 0, telling CI the server had started. The exit code is
    // the caller's to set, so the caller has to be told.
    return 1;
  }

  // Is there a qfs? Asked ONCE, here, and never fatal.
  //
  // A missing qfs must NOT stop the boot: markdown browsing — the demo's
  // second leg — needs no qfs at all, and refusing to start would withhold
  // the whole product over the half of it that is unavailable. But saying
  // nothing is the worse failure, and it was this build's behaviour: with no
  // qfs on PATH the viewer started clean, looked healthy, and the reader
  // learned otherwise only by clicking a qfs path and reading a spawn
  // ENOENT — the OS's words for "no such file", which name neither what was
  // missing nor how to get it.
  //
  // So: one probe, one line, then serve regardless. The advice is the
  // domain's (`unreachableAdvice`), identical to what a failed query
  // reports, and the version on the happy path is the only record of WHICH
  // qfs answered.
  //
  // Asked BEFORE the corpus source is attached, and of the connection rather
  // than of either arm, because it is now the one question both arms depend
  // on: the collection arm cannot enumerate without a qfs, and the scan arm
  // still serves every qfs column. `collection.read` below reports whether
  // THIS tree has a binding; this line reports whether there is a qfs to ask
  // at all, and the two failures read differently on purpose.
  const probe = probeQfs(config.content.qfs);
  logEvent(
    isOk(probe)
      ? {
          event: "qfs.ready",
          form: config.content.qfs.__tag,
          version: probe.content,
        }
      : {
          event: "qfs.unreachable",
          form: config.content.qfs.__tag,
          message: probe.content.content.message,
        },
  );

  // The connection is built ONCE, here, from the configured issuance form —
  // the same runner serves the corpus source (in collection mode) and every
  // qfs surface of the API.
  const runner = qfsRunner(config.content.qfs);
  const collection = config.content.collection;

  const ref: IndexRef =
    collection.__tag === "Some"
      ? attachCollection(
          runner,
          fs,
          collection.content,
          startedAt,
        )
      : attachScan(
          fs,
          cwd,
          config.content,
          startedAt,
        );

  pipe(
    // The writer is handed in HERE, at the composition root, and nowhere else.
    // `serve` is the local developer's surface — the mission's "editable in
    // place" — so this is the one place that grants write authority. A hosted
    // deployment builds its app without it and is read-only by construction
    // rather than by configuration.
    //
    // `readOnly` is that deployment, now reachable: it withholds the argument
    // instead of adding a check, so `/edit` does not exist rather than existing
    // and refusing. The distinction is the whole point of `edit` being an
    // argument — a check can be got wrong, a capability that was never granted
    // cannot.
    //
    // It exists because serving a corpus is not always serving YOUR corpus.
    // Pointed at another repository, a writable server is a browser UI with
    // commit rights over someone else's tree — and RBAC is OPEN when no
    // principals are declared, which is exactly the case for a repository that
    // has never heard of this tool. `--read-only` is what makes "run it at any
    // directory" safe to mean literally.
    api(
      ref,
      readOnly
        ? undefined
        : { fs, writer: nodeFileWriter(cwd) },
      config.content,
      // Granted UNCONDITIONALLY on the local serve surface, built from the
      // configured connection (the plan's issuance forms; spawn-on-demand by
      // default). This deliberately widens the old rule ("only where
      // resources are declared"): the mission's generic browsing makes every
      // path qfs can describe browsable with zero config, and qfs itself is
      // the authority on what this operator may reach — no credentials, no
      // rows. The reach stays read-only by construction (describe is pure;
      // `run` is never passed `--commit`), and the capability stays an
      // argument: a hosted build that omits it CANNOT reach qfs, exactly as
      // before. The SAME runner value serves the collection corpus source,
      // so there is exactly one qfs connection per server.
      runner,
    ),
    toFetch,
    serve({ port }, () =>
      logEvent({
        event: "serve.listening",
        url: `http://localhost:${port}/api/health`,
        // Said out loud, because "it cannot write" is the claim most worth
        // being able to check from outside. A read-only server that looks
        // identical to a writable one in its logs is one nobody can verify —
        // and this flag exists precisely for the case where the answer matters
        // to someone who is not the person who started it.
        readOnly,
      }),
    ),
  );
  return 0;
};
