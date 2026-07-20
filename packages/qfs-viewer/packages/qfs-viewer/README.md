# qfs-viewer

> **UNSTABLE** — Experimental study work. Part of the
> [qfs-viewer repository](../../README.md).

The markdown knowledge browser: it scans a repository's markdown, indexes the
front matter on memory, and serves it three ways from one model — SSR HTML for
people, a REST API for programs, and an MCP server for agents.

```sh
npx qfs-viewer        # at a repository root
```

> The npm package is **`qfs-viewer`**, lowercase. npm rejects uppercase
> package names outright, so `npx qfs-viewer` cannot resolve.
> "qfs-viewer" is the repository and product name; `qfs-viewer` is what
> you install.

## How it's organized

The layout is `domain/` + `vendors/` + `entrypoints/` — see
[ADR 0004](../../docs/adr/0004-package-layout-domain-vendors-entrypoints.md).

- **`src/domain/model/`** — the vocabulary as types. `DocumentPath`,
  `DocumentSlug`, `HeadingAnchor`, and `Route` are `refinedBrand`s, so a value
  only exists having passed its predicate at a boundary, and a `Route` cannot
  be passed where a `DocumentPath` is meant.
- **`src/domain/usecase/`** — pure logic. The scan, the index, and the query
  land here.
- **`src/vendors/`** — the anti-corruption boundary: `node:*` and any
  third-party adapter. The fs walk and the watcher land here.
- **`src/entrypoints/`** — thin program checkpoints: the CLI today; the SSR
  router, REST, and MCP as they land. Each calls the same domain procedures —
  that they can is the evidence the separation held.
- **`src/index.ts`** — the public barrel. Re-exports the domain only, never
  `vendors/`.

Third-party and `node:*` imports may appear **only** under `vendors/` or
`entrypoints/`; plgg-family specifiers are domain vocabulary, importable
anywhere. `../../scripts/gate-vendor-boundary.sh` enforces this.

## Dependencies

This package consumes the plgg family from the npm registry as published
`^version` dependencies and takes **no other runtime dependency**
([ADR 0001](../../docs/adr/0001-npm-only-plgg-family-contract.md)). Only what
is actually imported is declared — today that is `plgg` alone; the rest arrive
with the tickets that import them.

**plggmatic is deliberately not a dependency**
([ADR 0002](../../docs/adr/0002-plggmatic-is-a-reference-not-a-dependency.md)):
its column-accretion model is a design reference, reimplemented here on
`plgg-view`.

## Development

From the repository root:

```sh
./scripts/npm-install.sh          # install
./scripts/check-all.sh            # the one reproducible gate
./scripts/tsc-qfs-viewer.sh   # typecheck
./scripts/test-qfs-viewer.sh  # typecheck + unit suite (coverage-gated)
./scripts/format.sh               # Prettier (printWidth 50)
```

The build tools currently run via `../../scripts/plgg-tool.sh` rather than
`npm run build` / `npm run test`. That is a dated bridge around a Node 24
type-stripping fix that this environment's supply-chain policy hides until it
ages in — see
[ADR 0005](../../docs/adr/0005-pinned-toolchain-under-min-release-age.md),
which carries the retirement schedule.
