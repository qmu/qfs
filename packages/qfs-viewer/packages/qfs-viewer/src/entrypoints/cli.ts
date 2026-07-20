// The `qfs-viewer` command — a thin program checkpoint.
//
// An entrypoint is a shell: it reads argv, calls inward, and formats the
// result (workaholic:implementation / anti-corruption-structure). It holds no
// domain logic, which is why `node:*` may appear here and nowhere under
// `domain/`.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { serveCorpus } from "#qfs-viewer/entrypoints/serve";

const packageRoot = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
);

// The version is read from the manifest rather than duplicated in source: two
// copies drift, and the smoke check would then pass against a stale constant.
const version = (): string => {
  const raw: unknown = JSON.parse(
    readFileSync(
      join(packageRoot, "package.json"),
      "utf8",
    ),
  );
  return typeof raw === "object" &&
    raw !== null &&
    "version" in raw &&
    typeof raw.version === "string"
    ? raw.version
    : "unknown";
};

const usage = (): string =>
  [
    "qfs-viewer — a markdown knowledge browser on the plgg family",
    "",
    "Usage:",
    "  npx qfs-viewer serve [--port <n>] [--read-only]",
    "                                          scan this repository and serve it",
    "  npx qfs-viewer mcp                 serve it to an agent over MCP (stdio)",
    "  npx qfs-viewer --version            print the version",
    "  npx qfs-viewer --help               print this message",
    "",
    "`--read-only` serves the corpus WITHOUT the authority to change it: no",
    "/edit, because the writer is never built rather than because a check says",
    "no. Use it whenever the directory is not yours to edit — a writable server",
    "on someone else's tree is a browser with commit rights, and access control",
    "is OPEN until a repository declares principals.",
    "",
    "Run it at a repository root. `serve` reads the corpus — from qfs's",
    "markdown collection path (/markdown/<name>/documents|links) when",
    "qfs-viewer.config.json declares `collection`, else by scanning the",
    "working tree — then serves it as JSON:",
    "",
    "  GET /api/health                 the corpus at a glance",
    "  GET /api/documents              every document, path-ordered",
    "  GET /api/documents/<path>       one document's source",
    "  GET /api/errors                 documents the scan could not read",
    "",
    "It also serves the corpus as SSR HTML: `/` is the column browser, faceted",
    "by front-matter tag group, and `/<path>` is one rendered document with its",
    "headings numbered from its own outline.",
    "",
    "`mcp` speaks the Model Context Protocol on stdio, exposing the SAME index:",
    "list_documents, get_document, list_tag_groups, corpus_health. It is",
    "read-only until principals and RBAC exist.",
  ].join("\n");

const args = process.argv.slice(2);

/**
 * Start the MCP surface, importing it only when it is asked for.
 *
 * A DYNAMIC import, and not for startup time. `plgg-mcp` reaches
 * `plgg-content`, which imports `node:sqlite` at module load — and bun has no
 * such built-in, so a static import here made `qfs-viewer --version` fail
 * outright on bun ("No such built-in module: node:sqlite"). On node it merely
 * printed an ExperimentalWarning on every invocation, including the ones that
 * touch no database.
 *
 * So the cost of the MCP surface is now paid by the MCP surface. `serve`,
 * `--version` and `--help` never load it, which is both the honest
 * arrangement and the one that lets this run somewhere other than node.
 * `qfs-viewer mcp` still needs a runtime with `node:sqlite` until plgg-mcp
 * stops pulling its content tools in unconditionally — see
 * `.workaholic/tickets/todo/a-qmu-jp/20260715223100-mcp-drags-in-a-sqlite-warning.md`.
 */
const startMcp = (): void => {
  void import("#qfs-viewer/entrypoints/mcp").then(
    (m) => {
      m.serveMcp(process.cwd());
    },
  );
};

// `--port 4100` — the mission's gate port is the default, so a bare `serve`
// lands where the acceptance check looks.
const portArg = (): number => {
  const i = args.indexOf("--port");
  const raw = i === -1 ? undefined : args[i + 1];
  const parsed =
    raw === undefined ? NaN : Number(raw);
  return Number.isInteger(parsed) &&
    parsed > 0 &&
    parsed < 65536
    ? parsed
    : 4100;
};

// `--read-only` — serve a corpus WITHOUT the authority to change it.
//
// The flag is the safe direction by omission: forget it and you get the
// editable developer surface you asked for at your own repository, which is the
// default the mission wants. Pass it and the writer is never constructed.
//
// It is what makes "run it at any directory" honest. Pointed at a repository
// the runner does not own, an editable server is a browser with commit rights
// over someone else's tree — and principals are OPEN when none are declared,
// which any repository that has never heard of this tool satisfies.
const readOnlyArg = (): boolean =>
  args.includes("--read-only");

const main = (): number =>
  args[0] === "serve"
    ? serveCorpus(
        process.cwd(),
        portArg(),
        readOnlyArg(),
      )
    : args[0] === "mcp"
      ? (startMcp(), 0)
      : args.includes("--version") ||
          args.includes("-v")
        ? (process.stdout.write(`${version()}\n`),
          0)
        : args.includes("--help") ||
            args.includes("-h") ||
            args.length === 0
          ? (process.stdout.write(`${usage()}\n`),
            0)
          : (process.stderr.write(
              `unknown argument: ${args.join(" ")}\n\n${usage()}\n`,
            ),
            2);

process.exitCode = main();
