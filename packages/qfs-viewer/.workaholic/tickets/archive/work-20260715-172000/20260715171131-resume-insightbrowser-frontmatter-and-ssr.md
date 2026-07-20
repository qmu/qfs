---
created_at: 2026-07-15T17:11:31+09:00
author: a@qmu.jp
type: housekeeping
layer: [Config]
effort:
commit_hash:
category:
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Resume: retire the ADR 0005 toolchain bridge as each pin ages in

## CLOSED 2026-07-15 21:xx — superseded by events, not by completion

Both tickets this one existed to unblock are DONE and archived: the front-matter
index (`…004235`) and SSR with heading numbering (`…004236`). The dated
toolchain retirement this ticket owned is **not** done and has moved, so it is
carried where it will actually be read rather than in a todo nobody opens:

- **`docs/adr/0005`'s retirement schedule** now owns every remaining step, with
  the corrected dates. The `NPM_CONFIG_MIN_RELEASE_AGE=0` override in
  `scripts/smoke-npx.sh` is first to go — **2026-07-22 21:09 JST**, not the
  2026-07-16 this ticket assumed, because adopting plgg-md 0.0.3 restarted the
  seven-day floor. That ADR also records when to STOP extending the date rather
  than bump it again.
- **plgg-test ^0.0.5 / plgg-bundle ^0.0.6 and deleting `scripts/plgg-tool.sh`**
  remain open, on their own dates, in that same schedule.

What this ticket got WRONG, recorded so it is not repeated: its step 2 told the
next session to bump `plgg-view` to `^0.0.2` alongside plgg-md. Done as written
that breaks the build — but the fix was not "leave plgg-view alone" as a later
amendment to this ticket claimed. `plgg-server@0.0.4` (published 2026-07-09)
already pinned `plgg-view ^0.0.2`; nobody had looked. Both moved together and
the diamond dissolved.

## Overview

**Carry Origin:** session handoff on `work-20260715-003158` (now merged to `main` as [`cbbdc9d`](https://github.com/qmu/InsightBrowser/pull/1)) — carried on 2026-07-15 because the token window was filling; continue in a fresh session.

**This ticket does NOT supersede anything.** The two queued tickets — `20260715004235-markdown-scanner-and-frontmatter-index.md` and `20260715004236-ssr-browsing-and-heading-auto-numbering.md` — remain the owners of their own work and already carry their blockers, remaining steps, and risks in detail. This ticket covers only the **toolchain retirement** recorded in `docs/adr/0005-pinned-toolchain-under-min-release-age.md`, which lives in no other ticket, and it exists to be run **before** ticket `…004235` so the front-matter work starts on a consumable `plgg-md`.

### What is already done (context — do not redo)

The mission shipped its first branch. `main` carries a working product at **2/17** acceptance criteria:

- Repository skeleton, the vocabulary as plgg `refinedBrand` types, and two self-testing gates (dependency contract; vendor boundary) — [`611f685`](https://github.com/qmu/InsightBrowser/commit/611f685).
- Whole-tree markdown scan → immutable on-memory index, with skip-and-collect error handling — [`580a157`](https://github.com/qmu/InsightBrowser/commit/580a157), [`1fe03d2`](https://github.com/qmu/InsightBrowser/commit/1fe03d2).
- REST API + `serve` verb over that index — [`f5c32d0`](https://github.com/qmu/InsightBrowser/commit/f5c32d0).
- Hot reload wired to `node:fs.watch` — [`10e61f4`](https://github.com/qmu/InsightBrowser/commit/10e61f4).
- Container workload + `scripts/serve-development.sh` — [`b138ac5`](https://github.com/qmu/InsightBrowser/commit/b138ac5).
- Root page at `GET /` — [`2b79bce`](https://github.com/qmu/InsightBrowser/commit/2b79bce).
- Six ADRs, the branch story, and the deployment contract — [`e2416ca`](https://github.com/qmu/InsightBrowser/commit/e2416ca).

`./scripts/check-all.sh` exits 0 on `main` (67 tests, coverage >90% on all four metrics). Both worktrees are clean.

**Where work stopped:** nothing is mid-edit. The mission is stalled *only* on dated package availability — see `## Findings`.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — applies to all code work.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to all code work; no `any`/`as`/`!`/`@ts-ignore`.
- `workaholic:implementation` / `policies/command-scripts.md` — the retirement edits `scripts/build.sh` and `scripts/test-insightbrowser.sh`; CI must keep calling `check-all.sh` and nothing else.
- `workaholic:operation` / `policies/ci-cd.md` — the local gate is the authority; the hosted run is a fresh-clone backstop.
- `workaholic:implementation` / `policies/objective-documentation.md` — ADR 0005 carries the schedule; update it when each step lands rather than leaving it describing a past state.

## Implementation Steps

> **Each step is date-gated.** Run the availability check first and **stop** if the version is still hidden — a bump to a version `npm` cannot see fails the install and there is nothing to work around. `min-release-age=7` is a supply-chain control and must not be overridden (see `## Decisions`).

1. **Check what is consumable before touching anything.** Use a **dry-run install**, not `npm view` — see the correction in `## Findings`:
   ```sh
   cd "$(mktemp -d)" && echo '{"name":"probe","version":"1.0.0"}' > package.json
   npm install plgg-md@0.0.2 --dry-run     # repeat per package under test
   ```
   A dry-run that **resolves** is installable; `ETARGET` / `No matching version found … with a date before <cutoff>` means it is still inside the 7-day window. The error prints npm's live cutoff, which is the authoritative answer — read it rather than recomputing dates by hand. Do only the steps whose versions resolve, and leave the rest.
2. ~~**From 2026-07-16 18:11 JST — bump the render/parse pins.**~~ **DONE 2026-07-15, and step 2 as written was WRONG — do not re-run it.**
   - `plgg-md ^0.0.2` is bumped and the front-matter projection is built on it (commit `8c0d48c`). The bump used a per-command `NPM_CONFIG_MIN_RELEASE_AGE=0` at the developer's direction, reversing ADR 0005's decision 1; recorded in that ADR.
   - **`plgg-view` must STAY at `^0.0.1`.** Bumping it as this step directed **breaks the build**: `plgg-server@0.0.3` declares `plgg-view: ^0.0.1`, and a caret on a `0.0.x` version is an exact pin — so npm installs both copies, and 0.0.2's `Html` (which adds a `Raw` variant) will not assign to the `Html` that `pageResponse` expects. Nothing needs plgg-view 0.0.2; it can only move when a plgg-server that pins `^0.0.2` ships.
   - **First task of the 2026-07-16 18:11 drive: delete the `NPM_CONFIG_MIN_RELEASE_AGE=0` override from `scripts/smoke-npx.sh`** and confirm `./scripts/check-all.sh` stays green unaided. Until that is done, the smoke cannot see an unresolvable dependency. See ADR 0005's retirement schedule.
3. **From 2026-07-16 20:12 JST — retire the plgg-test half of the bridge.** Bump `plgg-test` to `^0.0.5` in `devDependencies`; its bin carries the Node-24 relocate fix. Change `scripts/test-insightbrowser.sh` to call `npm run test` in the package instead of `./scripts/plgg-tool.sh insightbrowser plgg-test`. Confirm `./scripts/check-all.sh` stays green.
4. **From 2026-07-20 11:38 JST — retire the rest and delete the bridge.** Bump `plgg-bundle` to `^0.0.6`; change `scripts/build.sh` to call `npm run build`; **delete `scripts/plgg-tool.sh`** — with both tools fixed it has no caller, and a workaround with no caller is debt. Confirm `./scripts/check-all.sh` stays green.
5. **Update `docs/adr/0005-pinned-toolchain-under-min-release-age.md`** as each step lands: mark the retirement steps done, and when step 4 completes change its status from `Accepted (time-boxed)` to `Superseded — retired 2026-07-20`, so the ADR describes the repository as it is.
6. **Then hand off to the real work:** drive `20260715004235-markdown-scanner-and-frontmatter-index.md` (front-matter half). Its own `## Progress` section lists its remaining steps and the `YamlMap` grammar risk to check first.

## Quality Gate

**Acceptance criteria:**

- `./scripts/check-all.sh` exits 0 after every individual bump — not merely at the end. A bump that reddens the gate is reverted, not carried forward.
- After step 3, `scripts/test-insightbrowser.sh` no longer references `plgg-tool.sh`, and the suite still reports the same test count and >90% coverage on all four metrics.
- After step 4, `scripts/plgg-tool.sh` **does not exist**, and `grep -rn "plgg-tool" scripts/ packages/` returns nothing outside `docs/adr/0005`.
- `min-release-age` is **not** overridden anywhere: `~/.npmrc` is unchanged, and no `--before` / `NPM_CONFIG_BEFORE` appears in any script or CI file.
- `scripts/gate-dependencies.sh` still passes: every runtime dep is plgg-family, none is plggmatic.

**Verification method:**

- `./scripts/npm-install.sh && ./scripts/check-all.sh` — exit 0 after each step.
- `git diff` on `~/.npmrc` is empty (it is outside the repo; confirm by reading it).
- `grep -rn "plgg-tool" scripts/ packages/ .github/` after step 4.

**Gate:**

- `check-all.sh` green after each bump, the bridge deleted, and ADR 0005 updated to match reality.

## Findings

- **`npm view` is NOT a validity check for `min-release-age`, and this ticket originally said it was.** Corrected 2026-07-15 17:19 JST after re-verification: `npm view plgg-md@0.0.2 version` prints `0.0.2` — and so does every other blocked version — because `npm view` reads registry metadata directly, while `min-release-age` is applied by the **installer** during resolution. The original step 1 ("a version that prints is installable") would therefore have reported all six packages as consumable and sent the next session into six failing bumps. The check that actually binds is `npm install <pkg>@<ver> --dry-run` in a temp dir, which errors `ETARGET … No matching version found for plgg-md@0.0.2 with a date before 2026/7/8 17:19:36` — and that cutoff timestamp is npm's own live answer, better than any recomputed date.
- **Nothing in this mission is unblocked before 2026-07-16 18:11 JST.** Re-verified by dry-run install on 2026-07-15 17:19 JST (originally verified 17:12 JST). The publish times behind the dates were re-derived from `npm view <pkg> time --json` and all six match this ticket's JST values exactly: `plgg-md`/`plgg-view` 0.0.2 at `2026-07-09T09:11Z`, `plgg-test` 0.0.5 at `2026-07-09T11:12Z`, `plgg-cms` 0.0.2 at `2026-07-10T17:25Z`, `plgg-bundle` 0.0.6 at `2026-07-13T02:38Z`, `plggpress` 0.0.4 at `2026-07-13T09:54Z`. `min-release-age=7` in `~/.npmrc` hides releases younger than seven days, and the plgg family shipped a burst on 2026-07-09→13. Consumable dates (**UTC published + 7d, shown in JST**): `plgg-md 0.0.2` and `plgg-view 0.0.2` → **2026-07-16 18:11**; `plgg-test 0.0.5` → **2026-07-16 20:12**; `plgg-cms 0.0.2` → **2026-07-18 02:25**; `plgg-bundle 0.0.6` → **2026-07-20 11:38**; `plggpress 0.0.4` → **2026-07-20 18:54**.
- **Correction to the shipped PR comment and story:** those state these dates as bare `2026-07-16 09:11`-style values, which are **UTC**. An earlier session message rendered that as "09:11 (~1.5h away)" — wrong; it was ~25h away, and 09:11Z is 18:11 JST. Trust the JST values above; recompute rather than trusting any prose.
- **`plgg-md 0.0.1` cannot do the front-matter job at all.** Its model is `Frontmatter = { layout: Option<SoftStr> }`: `parseFrontmatter` detects a flat `layout:` marker and **discards the rest of the block**. No `data`, no `YamlMap`, no `YValue`. The four-seam `RenderOptions` and the `YamlMap` model described in ticket `…004235`'s Key Files were read from the plgg **monorepo source**, which is ahead of the registry — the published 0.0.1 exposes only `Highlighter` and `LinkResolver`. When the contract is "consume from the registry" (ADR 0001), plan against the **published** `.d.ts`.
- **`renderHeading` is module-private in the published plgg-md** (0 occurrences in its public `.d.ts`), confirming ticket `…004236`'s premise: heading numbering cannot be injected without an upstream seam.
- **The pinned build tools do not run on Node 24 as installed** (`ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING`) — the exact bug upstream's `relocate.mjs` exists to fix. `scripts/plgg-tool.sh` applies that remedy from outside; it masks no failure (build, typecheck, tests, coverage and the npx smoke all really run).
- **`^0.0.x` is an exact pin.** npm treats a caret on a `0.0.x` version as that patch only, so the container and the host resolve identical versions even though `min-release-age` exists only in the developer's `~/.npmrc`. This is why the image needs no npm config of its own.
- **Live probing catches what the unit tests miss.** Three real bugs shipped green in CI this session: 404s carried no `cache-control` (the test asserted it on a 200); plgg-server's *unmatched-route* 404 bypasses global `use()` middleware entirely; and `GET /` answered "Not Found" to every human who opened the URL. An entrypoint change is not done until its surface has been driven live.
- **The reload path needs the prune rule explicitly.** `node_modules/plgg/README.md` is a valid `DocumentPath`, so `applyChange` without `isPrunedPath` would inject every dependency's README into the corpus one watch event at a time. The walk's prune does **not** cover the reload path.
- **`cloudflared` 2026.2.0 exits on SIGHUP** rather than reloading config, taking the supervising `bash -c` wrapper with it — a HUP intended as a reload took ~34 tunnelled hostnames offline for ~90s on 2026-07-15. Restart, never HUP. Also: `pgrep -f "cloudflared tunnel run"` matches its own command line and will report a dead tunnel as alive. Recorded in `.workaholic/deployments/development-tunnel.md`.
- **`merge-pr.sh` cannot complete from a mission worktree.** It runs `git checkout main`, which fails with "'main' is already used by worktree" because the primary worktree holds `main`. The `gh pr merge` **succeeds first**, so the PR merges and the script then dies before printing `{"merged": true}` — it looks like a failed merge that actually landed. Sync `main` manually in the primary worktree (`git -C <primary> pull origin main`).

## Decisions

- **`min-release-age=7` was not overridden, and must not be.** It is a supply-chain control; disabling it to make a gate go green inverts the reason it exists, and it would be switched off in a config file nobody revisits. The cost — a stalled mission — was accepted instead (`docs/adr/0005`).
- **The plgg family is consumed from the npm registry, never from a sibling checkout** (`docs/adr/0001`). This is what makes the 7-day wait binding rather than routable-around, and it is deliberate: it forces upstream gaps to be fixed upstream.
- **The scan takes the whole tree minus `PRUNED_DIRECTORIES`, not an allowlist of roots.** The mission's 「.workaholic/とdocs/、packages/**など**にも散らばる」 names examples, not a boundary; an allowlist omitted this repository's own `README.md`.
- **plggmatic is a design reference, not a dependency** (`docs/adr/0002`), machine-checked by `scripts/gate-dependencies.sh`.
- **Nothing is cached** (`docs/adr/0003`) — every response carries `no-store`, enforced by one middleware rather than per handler. ETag was considered and declined.
- **The package layout follows the machine-checked gate** (`domain/`+`vendors/`+`entrypoints/`), diverging from `coding-standards`' stated wording (`docs/adr/0004`).
- **Observability ships the logs half only** (`docs/adr/0006`): the metrics/traces half needs OpenTelemetry, which ADR 0001 forbids. A known, accepted gap — not an oversight to "fix" by adding the dependency.
- **Only declared what is imported.** `plgg-cms` and `plggpress` are named in the mission's contract but are **not** in `package.json`, because nothing imports them yet (and `plgg-cms` is not consumable until 2026-07-18). Add each when its consumer lands.

## Considerations

- **A container is running** on host port 4100 from this session, serving the bind-mounted worktree, and `insight-browser.qmu.dev` routes to it. Stop it with `podman compose -f workloads/development/compose.yaml down` from the worktree, or leave it — it is a dev surface, not production.
- **Ticket `20260715004236` (SSR + heading numbering) needs a developer decision, not a date**, and `/drive` must not resolve it unattended: either file the `decorateHeading` seam into plgg-md upstream and accept that the release is then hidden for 7 more days, or take the fallback (own renderer over the public `parseBlocks`, available once 0.0.2 lands) — which is a **recorded rejected alternative**, so choosing it is a deliberate re-decision.
- **Check the `YamlMap` grammar against the real corpus before modelling anything on it** (ticket `…004235` step 4). `YValue` allows scalars, a sequence of scalars, and a **one-level** map only; duplicate keys are an error, not last-wins; dates must be quoted — and this repository's own tickets carry unquoted `created_at: 2026-07-15T17:11:31+09:00`. A whole-corpus parse is the cheapest check. If it rejects them, plgg-md must be extended upstream and the 7-day wait applies again.
- **The mission worktree is `.worktrees/build-insightbrowser-on-the-plgg-family/`** on branch `work-20260715-003158`, which is merged. A fresh `/drive` should branch from the updated `main`; the old branch's work is fully landed.
- **`npx insightbrowser` is not published.** The name is unclaimed on npm. The tunnel hostname is `insight-browser.qmu.dev` (hyphenated) while the package is `insightbrowser` — harmless, but `insight-browser` is still free if they should match; cheap to change before publish, expensive after.
