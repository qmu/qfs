---
created_at: 2026-07-15T22:30:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Make `npx insightbrowser` run on bun and deno, not only node

## RESOLVED for bun, 2026-07-15 — and the diagnosis in this ticket was WRONG

bun runs the product. `--version`, `--help`, `serve`, the column UI, a rendered
document with numbered headings — all verified against the PACKED tarball.

This ticket said the fix was to bundle the CLI and retire the alias hook. That
was over-engineering built on a wrong root cause. The actual causes were two,
and both were small:

1. **`tsconfig.json` did not ship.** `files` was `[dist, src, bin]`, and
   `relocate.mjs` copied only `package.json`. bun and deno resolve
   `insightbrowser/*` from the tsconfig's `paths`; node uses `bin/hook.mjs`.
   With no tsconfig in the relocated copy, bun failed at the first inward
   import — which LOOKED like "the alias needs a node-only hook" and is not.
2. **`node:sqlite`, via a static `plgg-mcp` import.** With the alias fixed, bun
   then died on `No such built-in module: node:sqlite`, which bun does not
   have. `cli.ts` imported `entrypoints/mcp` statically, so `--version` loaded
   plgg-mcp → plgg-content → `node:sqlite`. The MCP import is now DYNAMIC: the
   cost of the MCP surface is paid by the MCP surface.

`scripts/smoke-npx.sh` now runs the packed bin under **every** runtime it finds
and SKIPS the absent ones out loud. It checked node only, which is exactly why
bun stayed broken through a whole session of green gates.

**Still open: deno.** Its installer was refused here as unprompted runtime
bootstrapping. The tsconfig fix should cover deno for the same reason it
covered bun, but that is a PREDICTION, not a result — the smoke prints
`SKIP: deno`. Do not check the mission item off until a deno actually runs it.

**`insightbrowser mcp` still needs `node:sqlite`**, so that verb is node-only
until plgg-mcp stops pulling its content tools in unconditionally — ticket
`20260715223100`.

## Overview

The mission requires `npx insightbrowser` on **node, bun, and deno**. Measured
2026-07-15: node works, **bun does not**, deno untested (its installer was
refused in this environment — see `## Considerations`).

```
$ bun ./node_modules/.bin/insightbrowser --version
error: Cannot find module 'insightbrowser/entrypoints/serve'
       from '/tmp/plgg-relocate-insightbrowser-0.0.1-.../src/entrypoints/cli.ts'
```

## Findings

- **The self-alias is resolved by a NODE-ONLY hook.** `bin/insightbrowser.mjs`
  calls `register("./hook.mjs", …)` from `node:module`, which teaches node to
  resolve `insightbrowser/*` → `src/*`. bun has no equivalent, so every inward
  import fails at the first one. The alias is not a convenience here — it is
  how every file imports every other file.
- **The bin runs TYPESCRIPT SOURCE, not built JS.** `dist/entrypoints/` holds
  only `.d.ts`; there is no `cli.js`. `dist/index.es.js` is the *domain barrel*
  bundle (`src/index.ts` re-exports the domain only), so it is not a runnable
  CLI.
- **That is also why `relocate.mjs` exists.** Node 24 refuses to strip types
  under `node_modules`, so the launcher copies the package to `/tmp` and
  re-execs. Both hacks are consequences of the same choice: ship source, run it
  through node's stripper.
- **One fix retires both.** Bundle `entrypoints/cli.ts` to `dist/cli.js` with
  the plgg family external and everything else inlined, and point `bin` at it.
  Then there is no alias to resolve, no types to strip, no relocation, and all
  three runtimes just run JavaScript. It would also let
  `docs/adr/0005`'s `scripts/plgg-tool.sh` retirement complete.
- **`plgg-bundle` may not do multi-entry yet.** Its published 0.0.2 could not
  even be introspected here (it fails its own `--help` under node 24 without
  the relocate bridge). Check `plgg-bundle 0.0.6` first; if a second entry is
  unsupported, that is an upstream request, not a local workaround.

## Implementation Steps

1. Check whether `plgg-bundle 0.0.6` supports a second entry (`dist/cli.js`).
   If not, file it upstream via `/request` and stop.
2. Bundle `entrypoints/cli.ts` → `dist/cli.js`, plgg-family external.
3. Point `bin.insightbrowser` at `dist/cli.js`; drop `register`/`hook.mjs` and
   `relocate.mjs` from the launcher, or delete the launcher entirely if the
   bundle can carry the shebang.
4. Extend `scripts/smoke-npx.sh` to run the packed bin under **each** available
   runtime, skipping any that is absent with a printed SKIP — a silent skip
   would let this regress unseen.
5. Update `docs/adr/0005`: the relocate bridge's retirement completes here.

## Quality Gate

- `node`, `bun`, and `deno` each run `insightbrowser --version` and
  `insightbrowser --help` from the PACKED tarball.
- `insightbrowser serve` boots and answers `/api/health` under each.
- `grep -rn "node:module\|hook.mjs\|relocate" bin/ scripts/` returns nothing.
- `./scripts/check-all.sh` exits 0.

## Considerations

- **deno could not be installed here.** The install was refused as unprompted
  runtime bootstrapping, which was a fair call. Verifying this ticket needs
  either a permission for `deno.land/install.sh` or a preinstalled deno; do not
  mark the mission item met on node+bun alone.
- **bun was installed** (1.3.14, `~/.bun/bin/bun`) and is what produced the
  failure above, so at least two of the three runtimes are testable today.
- The smoke check currently proves the packed bin runs under **node only**. It
  passed this whole time while bun was broken, which is exactly the blind spot
  step 4 closes.
