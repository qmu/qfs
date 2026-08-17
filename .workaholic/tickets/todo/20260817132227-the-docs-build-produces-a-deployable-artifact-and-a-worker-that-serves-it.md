---
created_at: 2026-08-17T13:22:27+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [20260817110309-the-docs-site-production-build-fails-on-blueprint-md.md]
mission: the-docs-site-publishes-itself-staging-on-merge-to-main-production-on-release
merge_policy:
verification_handoff: Deploying the worker needs the Cloudflare account token for the qmu.co.jp zone, which an unattended run does not hold
---

# The docs build produces a deployable artifact and a worker that serves it

## Overview

Nothing in this repository turns `docs/` into something that can be served off a developer's
machine. `package.json` defines `docs:build` (`vitepress build docs`), but no worker, no
`wrangler.toml`, and no publish command exist — `Dockerfile.docs`/`docker-compose.yml` run
`vitepress dev` only, and the sole outside exposure is a personal cloudflared tunnel whitelisted
in `docs/.vitepress/config.mts` (`qfs-guide.qmu.dev`). This ticket builds the deployable half of
the mission — the static artifact and the worker that serves it, parameterized by environment —
so the two later tickets only have to add their triggers.

It carries no trigger and no live deploy of its own: it must be provable with
`wrangler deploy --dry-run` and a local preview, which is what keeps the two workflow tickets
small.

Note the mission's prerequisite: `npm run docs:build` currently **fails** on `docs/blueprint.md`
(todo ticket `20260817110309-the-docs-site-production-build-fails-on-blueprint-md.md`). That fix
is queued separately and is not re-proposed here; this ticket assumes it lands first and should
not work around it.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` / `policies/ci-cd.md` — a check that is not automated is not a check
- `workaholic:implementation` / `policies/objective-documentation.md` — the docs are a deliverable

## Key Files

- `package.json` — `docs:build` already exists and produces `docs/.vitepress/dist`; the deploy
  scripts belong beside it.
- `docs/.vitepress/config.mts` — the `allowedHosts: ['qfs-guide.qmu.dev']` tunnel whitelist, and
  wherever a canonical site URL / `base` would have to be set for a deployed origin.
- A new `wrangler.toml` (or `wrangler.jsonc`) at the repository root — the worker declaration and
  its per-environment names and routes. It must not collide with `packages/qfs/crates/host/`,
  which *generates* wrangler config for the parked qfs-host Workers entrypoint and is unrelated.
- `.gitignore` — the build output directory must stay untracked.
- `docker-compose.yml`, `Dockerfile.docs` — the dev path stays as it is; only note that a
  production build now exists beside it.

## Implementation Steps

1. Run `npm install && npm run docs:build` and record what it emits and where
   (`docs/.vitepress/dist`), including whether the output is fully relative-path safe when served
   from a domain root.
2. Add the worker declaration: a static-assets worker (Cloudflare's `assets` binding) pointing at
   the build output, with two named environments — `staging` and `production` — differing only in
   worker name and route/custom domain (`staging-qfs.qmu.co.jp` / `qfs.qmu.co.jp`).
3. Add the npm scripts the two later tickets will call, e.g. `docs:deploy:staging` /
   `docs:deploy:production`, each running `docs:build` first so a broken build can never publish.
4. Prove it without credentials: `npx wrangler deploy --dry-run --env staging` (and `production`)
   exits 0 and reports the expected asset count; `npm run docs:preview` serves the built output
   locally and the site's nav, search, and generated reference pages render.
5. Record in `CLAUDE.md`'s Deploy section that the docs now have a publish path, so the next
   reader does not rediscover it from the workflow files.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `npm run docs:build` exits 0 and writes a complete static site (index, guide, cookbook, and the
  three generated reference pages all present in the output).
- `npx wrangler deploy --dry-run` exits 0 for both the `staging` and the `production` environment,
  with no credential present.
- No token, account id, or zone id is committed; every secret is referenced by name only.
- The deploy scripts run the build first, so a failing build cannot publish.

**Verification method** — the commands/tests/probes that prove them:

- `npm run docs:build`, exit code and an `ls` of the output tree recorded.
- `npx wrangler deploy --dry-run --env staging` and `--env production`, both exit 0.
- `npm run docs:preview` plus a browser check of `/`, `/guide/getting-started`, `/language`.
- `bash scripts/check-no-live-credentials.sh` exits 0 (the repository's own credential-shape gate).

**Gate** — what must pass before approval:

- `npm run docs:build` exits 0.
- `npx wrangler deploy --dry-run` exits 0 for both environments.
- `cd packages/qfs && cargo run -p xtask -- gen-docs --check` exits 0 (no generated page touched).

## Open Decisions

- **One worker with two environments, or two separate workers.** Options: a single
  `qfs-docs` worker with `[env.staging]`/`[env.production]` sections, versus two independently
  named workers. The first keeps one declaration and one rollback story; the second isolates a
  bad staging deploy from production's configuration entirely. Neither is clearly better without
  knowing how the Cloudflare account is organized, which this session cannot see.
- **Where the Cloudflare account and DNS live.** The ask names `staging-qfs.qmu.co.jp` and
  `qfs.qmu.co.jp` but not the account, the zone, or whether `qmu.co.jp` is served by Cloudflare
  DNS at all. If it is not, the custom-domain binding in steps 2 and 4 becomes a route on a
  proxied CNAME instead, which is a different configuration. The operator resolves this.

## Considerations

- The existing `allowedHosts: ['qfs-guide.qmu.dev']` entry documents a manual tunnel that these
  deployments supersede. Removing it is probably right, but it belongs to whoever still uses that
  tunnel — surface it, do not silently drop it.
- `ignoreDeadLinks: true` in the VitePress config means a deployed site can ship broken links
  silently. Related to the queued docs-build ticket's step 5; do not decide it twice.
