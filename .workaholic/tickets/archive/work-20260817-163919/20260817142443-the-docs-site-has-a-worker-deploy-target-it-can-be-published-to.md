---
created_at: 2026-08-17T14:24:43+00:00
status: done
author: noreply@anthropic.com
assignees: [a@qmu.jp]
depends_on:
mission: the-documentation-site-publishes-itself-staging-on-merge-production-on-release
merge_policy:
verification_handoff: publishing to a Cloudflare account and creating the two qmu.co.jp hostnames needs credentials an unattended run does not hold
---

# The docs site has a worker deploy target it can be published to

## Overview

PROPOSED. The ask names "a worker" as the mechanism that serves both hostnames, and this
repository has no deploy target for the docs site at all today — `npm run docs:build` writes
a static bundle that only CI's `docs-build` job ever looks at, and it looks at it only to
prove the pages compile. This ticket is the target itself: one Cloudflare Workers
static-assets project serving `docs/.vitepress/dist`, with two environments (staging and
production) so the two later tickets have something to publish *to*. It stops at a
deploy a person can run by hand; the two triggers are the tickets after it.

Cloudflare is read off the repository rather than chosen here: the dev docs host is already
reached through a cloudflared tunnel (`docs/.vitepress/config.mts`), `qfs-host` targets
Workers and generates a `wrangler.toml`, and qfs ships a `/cloudflare` driver. If the
account is not Cloudflare, this is the ticket where that is corrected, and the two trigger
tickets are unaffected — they call whatever command this one establishes.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `package.json` — holds `docs:build`; the deploy command joins it here.
- `docs/.vitepress/config.mts` — the site config; the build output path is derived from it.
- `.github/workflows/ci.yml` (`docs-build` job) — already runs the production build; the
  deploy tickets extend this, so the target must accept the same artifact.
- **New**: the worker config (e.g. repo-root `wrangler.toml` or `docs/wrangler.toml`) —
  note `packages/qfs/crates/host/fixtures/wrangler.golden.toml` is the *generated qfs-host*
  template and must not be confused with, or overwritten by, the docs site's own config.

## Implementation Steps

1. Confirm the build output directory VitePress writes (`docs/.vitepress/dist`) and that
   `npm run docs:build` produces a complete static tree from a clean checkout.
2. Add a worker config declaring the static assets directory and two named environments —
   staging and production — differing only in name and route/custom domain.
3. Add the deploy command(s) to `package.json` (e.g. `docs:deploy:staging` /
   `docs:deploy:production`) so both later tickets and a human invoke one thing.
4. Record which account-level inputs the deploy needs (API token scope, account id) and
   where they will live as GitHub Actions secrets — named, not embedded.
5. Hand off: publish once by hand to each environment and attach the two custom domains
   (staging-qfs.qmu.co.jp, qfs.qmu.co.jp), including their DNS records.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A single documented command publishes the built site to a named environment.
- The two environments are declared separately, and neither can be deployed by accident
  when the other was meant.
- No token or account id is committed; only the names of the secrets are.

**Verification method** — the commands/tests/probes that prove them:

- `npm run docs:build` from a clean checkout, then a dry-run of the deploy command
  (`wrangler deploy --dry-run` or equivalent) against each environment.
- `git grep` for the token/account-id values proves they are absent from the tree.

**Gate** — what must pass before approval:

- The existing CI is unchanged and green (this ticket adds config, not triggers).
- The by-hand publish to both hostnames is confirmed by the assignee (see
  `verification_handoff`).

## Considerations

- **Open Decisions** are carried in the staging ticket (access restriction on the staging
  hostname); this ticket must not resolve it silently — declare the environment, leave the
  restriction to that ticket.
- Cloudflare Workers static assets and Cloudflare Pages both serve this; Workers is chosen
  because the ask says "a worker" and because the rest of the project already targets the
  Workers runtime. If Pages is preferred, only this ticket changes.

## Final Report

Development completed as planned. `docs/wrangler.toml` declares a Cloudflare Workers
static-assets target for the VitePress build with two named environments, and
`package.json` carries the three commands (`docs:deploy:staging`,
`docs:deploy:production`, `docs:deploy:dry-run`) that both a human and the two trigger
tickets invoke. `wrangler` is a devDependency, so the deploy command is the same one
locally and in CI, pinned by `package-lock.json`.

The config was placed at `docs/wrangler.toml` rather than the repository root, of the two
options the ticket offered: asset paths resolve relative to the config file, so the site's
config sits next to the site, and the root stays free for the `qfs-host`-generated
`wrangler.toml` whose template is `packages/qfs/crates/host/fixtures/wrangler.golden.toml`.

The by-hand first publish and the two custom domains remain with the assignee — the
ticket's declared `verification_handoff`.

### Discovered Insights

- **Insight**: Wrangler resolves `assets.directory` relative to the *config file's*
  directory, not the working directory or `--config`'s caller. `docs/wrangler.toml` therefore
  says `directory = ".vitepress/dist"`, and the command can be run from the repository root.
  **Context**: Moving this file later without rewriting that path silently changes which tree
  gets published — the dry-run reports the file count it read, which is the cheapest way to
  catch it (`142 files` here, `0` if the path is wrong).

- **Insight**: `assets` is declared per environment even though wrangler would inherit the
  top-level block. The redundancy is the safety property the ticket asked for: an environment
  that names its own assets directory cannot silently pick up a different one if the top-level
  block is ever changed for the routeless default worker.
  **Context**: The top-level `qfs-docs` worker deliberately carries no route, so a bare
  `wrangler deploy` publishes nothing that any hostname serves — that is what makes "neither
  environment can be deployed by accident" true by construction rather than by convention.
