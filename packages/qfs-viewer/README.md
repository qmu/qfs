# qfs-viewer

> **UNSTABLE** — Experimental study work. Primarily intended for our own
> projects, though publicly available.

A **markdown knowledge browser** built on the
[plgg](https://github.com/qmu/plgg) family. It scans the markdown scattered
across a repository — `.workaholic/`, `docs/`, `packages/`, and wherever else
it has accumulated — indexes the front matter in memory, and serves it three
ways from one model: **SSR HTML** for people, a **REST API** for programs, and
an **MCP server** for AI agents.

```sh
npx qfs-viewer        # at a repository root
```

## Two users

- **The local developer.** `npx qfs-viewer` at a repository root starts a
  server over the working tree: markdown is scanned at boot, the front matter is
  indexed on memory and hot-reloads on edit, and documents are browsable *and*
  editable in place.
- **The hosted reader.** The same SSR server runs on Cloudflare Worker + D1 or
  Lambda + EFS + sqlite, served with document data that was compressed,
  optimized, and RAG-indexed ahead of time — with configuration and documents
  offloaded to R2, and no caching (incident prevention).

## What it is for

- **A tag-organized knowledge base.** Documents are tagged in markdown front
  matter. A *tag group* declares its variations — ticket kind (feature / bugfix
  / refactor), activity time (code reading / design / implementation / bugfix) —
  so the corpus is navigable by dimension rather than by filesystem tree.
- **A format AI can reach.** The REST API and MCP server replace the
  qmu-co-jp → workaholic *sync* with a live MCP *reference*; browser AI reaches
  the same corpus over WebMCP, and an in-page Realtime API answers questions
  about — and edits — the open document by voice, given an `OPENAI_API_KEY`.
- **A column-accretion UI.** A page link resolves *sideways* into a new column,
  so how you traversed there stays legible both on screen and in the URL. This
  follows the idea plggmatic proposed — columns as projected depth, the
  navigable state living in the URL. The plggmatic UI engine lives in this
  repository as [`packages/plggmatic`](packages/plggmatic/) (ported when the
  retired `qmu/plggmatic` repository was split up) and is, per ADR 0002's
  2026-07-17 amendment, the package's UI engine: the strip renders through
  the engine's columns, sticky column headers, and chrome, consumed from the
  npm registry (`plggmatic ^0.2.0`) like every other plgg-family dependency.
- **A documentation-site engine.** The plgg and qfs docs sites are built on this
  mechanism; loading a plugin exposes its documentation through the MCP server,
  and arbitrary structure is declarable in a config file.

## Dependencies

qfs-viewer consumes the plgg family from the npm registry as published
`^version` dependencies — `plgg`, `plgg-view`, `plgg-md`, `plggpress`,
`plgg-cms`, `plggmatic` — and takes **no other dependency**. It runs on node,
bun, and deno.

## Development

```sh
./scripts/serve-development.sh    # run it in a container -> http://localhost:4100
./scripts/npm-install.sh          # install every package's dependencies
./scripts/check-all.sh            # the one reproducible gate
./scripts/format.sh               # Prettier (per-package printWidth 50)
```

`serve-development.sh` launches [`workloads/development/`](workloads/development/):
qfs-viewer serving **this repository's own corpus**, with the repo
bind-mounted — so editing any markdown on your host hot-reloads the served
index. Nothing needs installing on the host.

```sh
curl localhost:4100/api/health      # {"documentCount":24,"errorCount":0}
curl localhost:4100/api/documents   # every .md in the repo (build output excluded)
```

Without a container, the same thing:

```sh
./scripts/npm-install.sh
node packages/qfs-viewer/bin/qfs-viewer.mjs serve --port 4100
```

`check-all.sh` is the contract: the dependency-contract gate, the
vendor-boundary gate, the dist build, the `npx` smoke (pack → install → run the
real bin), then typecheck and the coverage-gated unit suite. CI calls it and
adds nothing.

## Status

**It runs.** `qfs-viewer serve` scans a repository, holds the corpus in an
immutable on-memory index, hot-reloads as the tree changes, and serves it as a
REST API and a root page listing every document.

| | |
| --- | --- |
| **Working** | the scan, the index, hot reload, the REST API, the root page, the `npx` launcher, the dependency + vendor-boundary gates |
| **Not built** | front-matter parsing and tag groups, server-rendered documents with heading numbering, the column-accretion UI, MCP, RBAC, voice, the hosted targets |

The two gaps are **blocked, not forgotten**: front matter needs a `plgg-md`
release that this environment's supply-chain policy hides until 2026-07-16
(see [ADR 0005](docs/adr/0005-pinned-toolchain-under-min-release-age.md)), and
document rendering waits on a decision recorded in its ticket. Until then the
root page links documents to their JSON and says so, rather than pretending.

- The mission and its queue: [`.workaholic/missions/`](.workaholic/missions/)
- The reasoning: [`docs/adr/`](docs/adr/index.md) — start with
  [0001](docs/adr/0001-npm-only-plgg-family-contract.md) (why npm, not a
  sibling checkout) and [0003](docs/adr/0003-no-caching.md) (why nothing is
  cached).
- The vocabulary: [`.workaholic/terms/`](.workaholic/terms/index.md)
