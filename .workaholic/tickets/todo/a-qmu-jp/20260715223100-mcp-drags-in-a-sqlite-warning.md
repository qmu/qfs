---
created_at: 2026-07-15T22:31:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# `qfs-viewer --version` now prints a SQLite experimental warning

## PARTLY RESOLVED 2026-07-15 — and it was never just a warning

The warning is gone from `--version`, `--help` and `serve`: `cli.ts` imports
`entrypoints/mcp` DYNAMICALLY now, so nothing that does not speak MCP loads
plgg-content.

**This ticket under-sold the defect.** It called it noise. On bun the same
static import was fatal — `No such built-in module: node:sqlite` — so
plgg-mcp's unconditional `plgg-content` import was not making the product
untidy, it was making the product unrunnable on a runtime the mission
requires. A warning on one runtime and a hard failure on another are the same
bug wearing different clothes.

**Still open, and still upstream-only:** `qfs-viewer mcp` loads
plgg-content and therefore needs a runtime with `node:sqlite` (node ≥22). The
subpath fix below is what makes the MCP verb cross-runtime too.

## Overview

Adding `plgg-mcp` made the product's headline command noisy:

```
$ node ./node_modules/.bin/qfs-viewer --version
(node:1559705) ExperimentalWarning: SQLite is an experimental feature and might change at any time
(Use `node --trace-warnings ...` to show where the warning was created)
0.0.1
```

The warning is emitted on EVERY invocation, including `--version` and
`--help`, which touch no database at all.

## Findings

- **Source:** `plgg-mcp` depends on `plgg-content` ("a derived, rebuildable
  SQLite index over a git-primary Markdown corpus"), which imports
  `node:sqlite` — see
  `node_modules/plgg-content/dist/Stakeholder/usecase/openStakeholderStore.d.ts`
  and its `index.es.js`. Node prints the warning at import time.
- **qfs-viewer uses none of it.** `plgg-content` arrives only because
  `plgg-mcp`'s `contentTools` (plggpress's `search_content`/`get_article`/
  `list_collections`) are built on it. This repository registers its OWN tools
  over its own on-memory index and never touches `plgg-content`.
- **So the cost is entirely incidental**: a transitive dependency on a SQLite
  store, loaded eagerly, for tools we do not use.

## Policies

- `workaholic:planning` / `policies/verify-before-building.md` — the "no local
  fix" verdict below is backed by the two commands that produced it (the
  `exports` map has one entry; `index.es.js` imports `plgg-content` at the top,
  unconditionally). Re-run both before accepting it: plgg-mcp may have
  published since, which would make this ticket a version bump rather than a
  wait. This branch has mistaken a stale check for a wall four times.
- `workaholic:implementation` / `policies/objective-documentation.md` — the
  correction at the top (a warning on node was a hard FAILURE on bun) stays
  attached rather than being tidied away; it is why this ticket outranked its
  own "just noise" framing.
- `workaholic:implementation` / `policies/coding-standards.md` — if a local
  workaround is ever attempted, it must not be `--no-warnings` /
  `NODE_NO_WARNINGS`: silencing every future warning to hide one is the
  escape-hatch move this repository forbids in every other form.
- `workaholic:design` / `policies/modular-monolith-first.md` — the ask upstream
  is a packaging split (`exports` subpaths), not a code change: a consumer
  registering its OWN tools should not inherit plggpress's content store.

## Implementation Steps

**There is no local fix. This is upstream-only — checked, not assumed:**

```
$ node -e "console.log(JSON.stringify(require('plgg-mcp/package.json').exports))"
{".":{"import":{...,"default":"./dist/index.es.js"},"require":{...}}}

$ head -8 node_modules/plgg-mcp/dist/index.es.js
import * as __ext2 from "plgg-content";     <- top level, unconditional
```

`plgg-mcp` publishes exactly ONE entry point, and that entry imports
`plgg-content` at the top of the file. So there is no subpath to import the
protocol core from, and no import order that avoids it: touching `plgg-mcp` at
all loads `node:sqlite`.

1. **File upstream via `/request`:** ask plgg-mcp to expose `contentTools`
   behind a subpath (`plgg-mcp/content-tools`, or an `exports` map with
   `./mcp`, `./transport`), so a consumer registering its OWN tools does not
   inherit plggpress's content store. It is a packaging change, not a code
   one, and it benefits any consumer that does what this repository does.
   Expect the usual 7-day floor after it publishes.
2. **Until then, accept the warning.** It is noise on stdout-adjacent stderr,
   not a fault: nothing is broken, and no data is touched. Do NOT paper over
   it — see Considerations.

## Quality Gate

- `node ./node_modules/.bin/qfs-viewer --version` prints exactly the
  version and nothing else — assert it in `scripts/smoke-npx.sh`, which today
  only compares stdout's version string and so did not catch this.
- `qfs-viewer mcp` still answers `initialize`/`tools/list`/`tools/call`.
- `./scripts/check-all.sh` exits 0.

## Considerations

- **The smoke check let this through.** It compares `--version` output to the
  manifest and passes because the warning goes to STDERR. Asserting a clean
  stderr on `--version` is the cheap fix and belongs in the same change.
- Do not silence it with `--no-warnings` or `NODE_NO_WARNINGS`: that hides
  every future warning too, including ones about this product's own code.
