---
created_at: 2026-07-15T00:42:34+09:00
author: a@qmu.jp
type: enhancement
layer: [Config, Domain]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Repository skeleton and the npm-only plgg-family dependency contract

## Overview

Stand up the InsightBrowser repository skeleton: the workaholic top-level layout, the first package on the gate-conformant internal layout, and **one reproducible `scripts/check-all.sh` gate**. The skeleton's job is to make the mission's central contract — *the plgg family from npm, and nothing else* — machine-checked from the first commit rather than promised in prose.

The repository currently holds only the bootstrap commit (`README.md`, `CLAUDE.md`, `.gitignore`). [plggmatic](https://github.com/qmu/plggmatic) is the near-exact template: a standalone repo consuming the plgg family from the npm registry as published `^version` dependencies, whose `check-all.sh` composes `build.sh` plus per-package `tsc`/`test` runners into a single gate.

Two facts were verified against the live registry at ticket time and decide the ticket's shape:

- **All five runtime dependencies are published**: `plgg` 0.0.27, `plgg-view` 0.0.2, `plgg-md` 0.0.2, `plggpress` 0.0.4, `plgg-cms` 0.0.2. The contract resolves — no sibling checkout is needed.
- **`npx InsightBrowser` cannot work.** npm rejects uppercase package names (`npm view InsightBrowser` returns *"This package name is not valid"*). The published package is therefore **`insightbrowser`** (verified available), invoked as **`npx insightbrowser`**. "InsightBrowser" survives as the repository/product name in prose only. The README, CLAUDE.md, and the mission's acceptance wording must be corrected to match.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — fixes the top level: `packages/` (one directory per package), `scripts/` (repo-wide `[verb]-****.sh`), `workloads/`, `docs/`, `outputs/` (gitignored). Names must be pronounceable words. The committed `.gitignore` already encodes the `outputs/` rule.
- `workaholic:implementation` / `policies/coding-standards.md` — binds every `packages/` TypeScript file: no `any`, `as`, non-null `!`, `@ts-ignore`, `null`, `==`, `class`, `enum`, `switch`, `var`, `this`. Receive at boundaries with `unknown`. Note the divergence recorded under Considerations.
- `workaholic:implementation` / `policies/command-scripts.md` — governs the single gate: CI **calls** `check-all.sh` rather than re-implementing it, so local and CI paths are identical. Every script opens `#!/bin/sh -eu` and re-roots via `git rev-parse --show-toplevel`.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — SSR, REST, and MCP are entry points and belong **outside** the domain as thin shells calling the same public procedures. Do not create a top-level `src/infrastructure/`.
- `workaholic:implementation` / `policies/objective-documentation.md` — the npm-only contract, the plggmatic exclusion, the no-cache rule, and the layout divergence are non-obvious decisions with alternatives considered; they require ADRs recording the **reasoning**, not README bullets.
- `workaholic:design` / `policies/sacrificial-architecture.md` — draw boundaries along units throwable away whole (scanner, index, renderer, each transport adapter); do not pre-optimize.
- `workaholic:design` / `policies/modular-monolith-first.md` — SSR + REST + MCP are three surfaces of **one** deployment unit; the future Worker/Lambda target does not license a split.
- `workaholic:planning` / `policies/terminology.md` — highest-leverage decision here: fix one word per concept now (*document*, *front matter*, *index*, *scan*) and use the identical term in TS types, REST paths, and MCP tool names.
- `workaholic:operation` / `policies/ci-cd.md` — hosted CI is a fresh-clone **backstop**; a green badge is never the primary health signal. plgg's `run-tests.yml` is the template.

## Key Files

Reference (do **not** edit — other repositories):

- `/home/ec2-user/projects/plggmatic/scripts/check-all.sh` - the exact gate shape to mirror: `#!/bin/sh -eu`, re-root, a comment block stating why the gate is the contract, ordered delegation to `build.sh` + per-package `test-*.sh`, closing success echo. It explicitly asserts a clean run proves the published cross-repo contract resolves — the same claim this gate makes.
- `/home/ec2-user/projects/plggmatic/scripts/npm-install.sh` - per-package install in dependency order.
- `/home/ec2-user/projects/plggmatic/scripts/build.sh` - builds each dist via plgg-bundle; in a standalone repo the build tool is itself an npm dep, so there is no `file:`-link bootstrap.
- `/home/ec2-user/projects/plggmatic/scripts/tsc-plggmatic.sh` - the 4-line per-package runner template.
- `/home/ec2-user/projects/plggmatic/scripts/format.sh` - single top-level Prettier invocation relying on per-package `.prettierrc.json` discovery.
- `/home/ec2-user/projects/plggmatic/packages/plggmatic/package.json` - the npm-contract template: `type: module`, dist ESM main/module, exports map, the seven-script block, plgg family as `^version` deps, `plgg-bundle`/`plgg-test`/`typescript` as devDeps.
- `/home/ec2-user/projects/plggmatic/packages/plggmatic/tsconfig.json` - strict-config template for a built package (`strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `erasableSyntaxOnly`, `verbatimModuleSyntax`, self-alias `paths`).
- `/home/ec2-user/projects/plgg/packages/plgg-md/bundle.config.ts` - plgg-bundle config shape (`alias: { prefix, srcRoot }` makes self-alias paths resolve at build).
- `/home/ec2-user/projects/plgg/packages/plgg-md/plgg-test.config.json` - coverage gate config (`threshold: 90`).
- `/home/ec2-user/projects/plgg/.workaholic/constraints/architecture.md` - **the** operative vendor-boundary contract; states plgg-family packages are *domain vocabulary, not vendors* — importable anywhere.
- `/home/ec2-user/projects/plgg/scripts/gate-vendor-boundary.sh` + `vendor-boundary-analyzer.mjs` - the gate to port; uses the already-present `typescript` package (zero new deps) and self-tests each run.
- `/home/ec2-user/projects/plgg/packages/plgg-cms/package.json` - closest whole-package analog; its `bin` + `files: [dist, src, bin]` is the shape `npx insightbrowser` needs.
- `/home/ec2-user/projects/plgg/packages/plggpress/bin/plggpress.mjs` - the npx launcher model, including the `relocateOutOfNodeModules` re-exec (Node 24 refuses to strip types from `.ts` under `node_modules`).

Target (in this repository):

- `CLAUDE.md` - already corrected on this branch to exclude plggmatic and name the five deps; keep it authoritative.
- `README.md` - corrected on this branch; must additionally reflect `npx insightbrowser`.
- `.gitignore` - already byte-identical to plggmatic's, including `**/package-lock.json` as environment-local.

## Related History

None — this is the repository's first implementation ticket. The historical precedent lives in the sibling repos: plggmatic's own repo-split (`docs/0001-plggmatic-repo-split.md`) established the standalone-repo/npm-contract pattern this ticket inherits.

## Implementation Steps

1. **Fix the package name across the repo.** Settle `insightbrowser` as the npm name and `npx insightbrowser` as the invocation. Correct `README.md`, and the mission's acceptance wording at `.workaholic/missions/active/build-insightbrowser-on-the-plgg-family/mission.md`.
2. **Create the top level** per `directory-structure`: `packages/`, `scripts/`, `docs/`, `workloads/`. (`outputs/` is gitignored and created at runtime.)
3. **Create `packages/insightbrowser/`** with `package.json` (name `insightbrowser`, `type: module`, `bin`, `files: [dist, src, bin]`, the five plgg deps at their verified `^versions`, devDeps `typescript`/`plgg-bundle`/`plgg-test`/`@types/node`), `tsconfig.json` (strict set + self-alias `paths`), `.prettierrc.json` (`printWidth: 50`), `bundle.config.ts`, `plgg-test.config.json` (`threshold: 90`), and `README.md`.
4. **Lay out `src/` on the gate-conformant layout**: `domain/{model,usecase}`, `vendors/`, `entrypoints/`. `index.ts` re-exports domain only, never `vendors/`. Colocate `*.spec.ts` beside source.
5. **Fix the vocabulary now** (`terminology`): one word per concept — *document*, *front matter*, *index*, *scan* — used identically in TS types, REST paths, and MCP tool names. Record the glossary in `.workaholic/terms/`.
6. **Write `scripts/`** on the plggmatic templates: `npm-install.sh`, `build.sh`, `tsc-insightbrowser.sh`, `test-insightbrowser.sh`, `format.sh`, and `check-all.sh` composing them. Every script `#!/bin/sh -eu`, re-rooted, closing with the success echo.
7. **Port `scripts/gate-vendor-boundary.sh`** + its analyzer from plgg. Third-party/`node:*` specifiers only under `vendors/` or `entrypoints/`; plgg-family importable anywhere as domain vocabulary. Wire it into `check-all.sh`.
8. **Write `scripts/gate-dependencies.sh`**: fail if any `package.json` `dependencies` block names plggmatic or any non-plgg-family package. devDependencies (`typescript`, `plgg-bundle`, `plgg-test`, `@types/node`) are exempt — the contract governs **runtime**. Wire it into `check-all.sh`.
9. **Add an `npx` smoke check**: `npm pack` then run the packed bin, asserting it resolves and runs from outside a `node_modules` type-stripping context (the plggpress trap). Wire into `check-all.sh`.
10. **Scaffold `.workaholic/`** subtrees the README already points at: `terms/`, `specs/`, `concerns/`, `deployments/`, `release-notes/`, each with an `index.md`; refresh the OKF indexes.
11. **Write the ADRs** under `docs/` (`objective-documentation` — reasoning, not just decision): (a) npm-only plgg-family contract and why plggmatic is excluded despite being the column-UI source; (b) the `domain/vendors/entrypoints` layout divergence from `coding-standards`' stated wording; (c) the **no-cache rule** — the mission asserts it, and no policy in the corpus backs it, so it binds only if written here (precedent to copy: `plgg-poc-portal`/`plgg-poc1-search` `entrypoints/serve.ts` send `cache-control: no-store, must-revalidate`); (d) the observability gap — `observability`'s metrics/traces half needs OpenTelemetry, which the no-dependency contract forbids; the structured-log half is satisfiable in-repo.
12. **Add CI** (`ci-cd`) as a fresh-clone backstop that calls `npm-install.sh` then `check-all.sh` and nothing else, on plgg's `run-tests.yml` template.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `./scripts/check-all.sh` exits 0 from a clean checkout after `./scripts/npm-install.sh`, proving the published cross-repo contract resolves against the real registry (`plgg` ^0.0.27, `plgg-view` ^0.0.2, `plgg-md` ^0.0.2, `plggpress` ^0.0.4, `plgg-cms` ^0.0.2).
- `scripts/gate-dependencies.sh` exits 0 on the conformant tree, and exits non-zero when a non-plgg runtime dependency or `plggmatic` is added to any `package.json` `dependencies` block (demonstrate both directions).
- `scripts/gate-vendor-boundary.sh` exits 0 on the conformant tree, and exits non-zero when a `node:*` or bare third-party import is introduced under `src/domain/` (demonstrate both directions). Its self-test passes on each run.
- `npm pack` produces a tarball whose `bin` resolves and runs: `npx <tarball>` prints a version and exits 0, from outside a `node_modules` type-stripping context.
- `tsc --noEmit` passes with zero errors under the strict set, with no `any`, `as`, `!`, or `@ts-ignore` anywhere in `packages/`.
- No `package.json` in the repo names `plggmatic` in `dependencies`.

**Verification method** — the commands/tests/probes that prove them:

- `./scripts/npm-install.sh && ./scripts/check-all.sh` — the single gate; exit 0.
- Both negative gate demonstrations run in-session (add the violating import/dep, observe non-zero exit, revert).
- `npm pack && npx ./insightbrowser-<version>.tgz --version` — the npx smoke.
- `grep -rn '\bany\b\|\bas \|@ts-ignore' packages/ --include='*.ts'` returns no escape-hatch usage.

**Gate** — what must pass before approval:

- `./scripts/check-all.sh` green, including the ported vendor-boundary gate, the dependency-contract gate, and the npx smoke.
- Both gates demonstrated failing on a deliberate violation and passing after revert — a gate never proven to fail is not a gate.
- The four ADRs exist under `docs/` and each records reasoning and alternatives, not only the decision.

## Considerations

- **The package name is settled but the mission text is not yet consistent.** `npx InsightBrowser` appears in the mission Goal/Scope and the README; step 1 must correct them or the acceptance criteria will read as unmet forever (`.workaholic/missions/active/build-insightbrowser-on-the-plgg-family/mission.md`, `README.md`).
- **"No other dependency" governs runtime only.** Every reference package carries `typescript`, `@types/node`, `plgg-bundle`, `plgg-test` as devDependencies and cannot build or test without them. The contract and `gate-dependencies.sh` must both state this explicitly, or the gate will reject a buildable repo (`packages/insightbrowser/package.json`).
- **The dependency closure is clean — verified.** `plggpress` and `plgg-cms` transitively pull only plgg-family packages (`plgg-http`, `plgg-server`, `plgg-sql`, `plgg-auth`, `plgg-kit`, `plgg-db-migration`, `plgg-highlight`, `plgg-cli`), so the five deps introduce no third party.
- **The layout diverges from the policy's stated wording** (`coding-standards.md` says `src/<Domain>/{model,service,dependency}`). No package in plgg or plggmatic uses that; the machine-checked gate enforces `domain/{model,usecase}` + `vendors/` + `entrypoints/`. Decision taken: follow the gate; record the divergence in ADR (b) so it is not re-litigated per package.
- **The no-cache rule has no policy behind it.** Grepped across the corpus, every `cache`/`stale` hit is an unrelated sense. It binds only as an InsightBrowser ADR (c) (affects every future `entrypoints/` handler).
- **`Result`/`Option` come from plgg, not hand-defined.** `domain-layer-separation` says the Result type is "defined yourself", while `anti-corruption`'s model layer permits "the project's base library" and plgg's constraints doc names `Option`/`Result`/`Str`/`Dict` as sanctioned boundary-crossing types. Write this into ADR (a) so it is not re-litigated per package.
- **`plgg-md` sits at 0.0.2 with `file:` deps inside the monorepo** but publishes 0.0.2 to the registry; the published artifact is what this repo consumes. Ticket 3 will need a *new* plgg-md release (see the heading-seam decision), so the `^0.0.2` floor will move.

---

**Next in this mission:** `20260715004235-markdown-scanner-and-frontmatter-index.md` (scanner + index), then `20260715004236-ssr-browsing-and-heading-auto-numbering.md` (SSR + numbering).

## Final Report

**Outcome:** Implemented. `./scripts/check-all.sh` is green end to end.

### What was built

- `packages/insightbrowser/` on the `domain/` + `vendors/` + `entrypoints/` layout (ADR 0004), with the strict tsconfig set, per-package `.prettierrc.json` (printWidth 50), `bundle.config.ts`, and a 90% coverage gate.
- **The vocabulary as types** (`src/domain/model/Vocabulary.ts`) — `DocumentPath`, `DocumentSlug`, `HeadingAnchor`, `Route` as plgg `refinedBrand`s, with the glossary recorded in `.workaholic/terms/index.md`. 100% covered.
- **The `npx insightbrowser` launcher** — `bin/insightbrowser.mjs` + `hook.mjs` + `relocate.mjs`, carrying the Node-24 relocation upstream needs.
- **Two machine-checked gates**: `gate-dependencies.sh` (runtime deps are plgg-family only, never plggmatic) and the ported `gate-vendor-boundary.sh` (third-party imports confined to `vendors/`/`entrypoints/`). Both self-test their own red/green logic on every run.
- **The npx smoke** (`smoke-npx.sh`) — packs, installs the tarball into a scratch tree, and runs the real bin from under `node_modules`.
- Six ADRs, the `.workaholic/` OKF subtrees, and CI calling `check-all.sh` and nothing else.

### Verification actually run

- `./scripts/check-all.sh` → exit 0: both gates green (12 + 6 self-test cases pass), dist built, npx smoke passed, `tsc --noEmit` clean, **10 tests passed, coverage 100%** on all four metrics.
- **Both gates demonstrated red, then reverted** — a gate never proven to fail is not a gate:
  - foreign runtime dep (`chokidar`) → `dependency-contract gate: FAILED … not a plgg-family package`
  - `plggmatic` as a dep → `FAILED … plggmatic is a design reference, not a dependency`
  - `node:fs` imported into `domain/usecase/` → `vendor-boundary gate: FAILED … packages/insightbrowser/src/domain/usecase/leak.ts:1 imports "node:fs"`
- The npx smoke is the check that earns its keep: it proves the packed bin runs from under `node_modules`, which no other check touches.

### Discovered insights

1. **`npx InsightBrowser` cannot work** — npm rejects uppercase package names (`This package name is not valid`). Verified against the live registry. The package is `insightbrowser`; corrected in the README, CLAUDE.md, and the mission.
2. **`min-release-age=7` (a supply-chain control in `~/.npmrc`) hides most of the plgg family.** The family shipped a release burst 07-09→07-13, so today only `plgg 0.0.27`, `plgg-bundle 0.0.2`, and `plgg-test 0.0.3` are installable — and **`plgg-cms` has no consumable version at all**. The control was **not** overridden: disabling a security control to make a build go green inverts its purpose. See ADR 0005 for the dated bridge and its retirement schedule.
3. **The pinned build tools do not run on Node 24 as installed** — `ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING`, the exact bug upstream's `relocate.mjs` exists to fix. `scripts/plgg-tool.sh` applies that same remedy from the outside; it is time-boxed, with deletion scheduled for 2026-07-20.
4. **`as` is banned, so branding needed the right idiom** — the first draft used `as` casts. plgg's `refinedBrand` produces a `Box`-branded value with no cast at all. The compiler caught the whole class immediately, which is the argument for the strict set.
5. **Only what is imported is declared.** The skeleton imports `plgg` alone, so `plgg` alone is declared. Pre-declaring the other four would have made the dependency gate meaningless — and would have been impossible anyway (`plgg-cms` is uninstallable today).

### Deviations from the ticket

- **Dependency pins**: the ticket specified all five runtime deps at their verified `^versions`. Shipped with `plgg ^0.0.27` only — the rest are neither imported yet nor installable under the release-age policy. The contract is unchanged; the gate enforces it.
- **`npm run build` / `npm run test`** route through `scripts/plgg-tool.sh` for now (ADR 0005). The package scripts stay pointed at the tools directly so the direct path works the moment the pins move.
