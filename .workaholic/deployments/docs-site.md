---
type: Deployment
title: qfs documentation site (staging on merge, production on release)
environment: production
confirmation_method: api-probe
url: https://qfs.qmu.co.jp
command: npm run docs:deploy:production
deploy_model: deploy-on-merge
paths: [docs/, package.json, scripts/stamp-docs-deploy.sh]
---

## Procedure

The documentation site publishes itself to **two** hostnames, from two different triggers.
Nobody runs a deploy command in the normal path; the by-hand commands exist for recovery.

| Environment | Hostname | Trigger | Workflow |
| --- | --- | --- | --- |
| staging | `https://staging-qfs.qmu.co.jp` | a push to `main` (every merge) | `.github/workflows/ci.yml`, job `docs-build` |
| production | `https://qfs.qmu.co.jp` | a pushed `v*` tag, after the Release publishes | `.github/workflows/release.yml`, job `docs-deploy-production` |

Both environments are Cloudflare Workers static-assets projects declared in
`docs/wrangler.toml` (`qfs-docs-staging`, `qfs-docs-production`), each reachable only through an
explicit `--env`; no environment carries a route, so a `wrangler deploy` without `--env` cannot
land on either hostname.

**The hostnames are attached by hand, not by the config.** Each Worker's custom domain
(`staging-qfs.qmu.co.jp`, `qfs.qmu.co.jp`) was attached once through the Cloudflare dashboard and
is deliberately **not** declared in `docs/wrangler.toml`. A declared `custom_domain` route is
re-asserted on every deploy, which would force the CI token to hold `Zone → Workers Routes → Edit`
on `qmu.co.jp` permanently — a scope that cannot be narrowed below the whole zone, so a leaked CI
token could attach a Worker to any hostname on it. Without the routes, the token needs account
scope only, and a leak is bounded to "the docs site's content can be replaced".

**Re-attaching a domain** (needed only if a Worker is deleted and recreated): Cloudflare dashboard
→ Workers & Pages → the Worker → Settings → Domains & Routes → Add custom domain. Doing it from
the CLI instead requires a token with the wide `Zone → Workers Routes → Edit` scope that the CI
secret deliberately no longer has.

**Which record applies to your change.** This one covers `docs/` and the site's build and
publish path. `github-release.md` covers the `qfs` binary — the versioned, installable
deliverable. A change that touches both (a version bump whose release notes cite a new docs
page) travels both, in that order: the tag fires `release.yml`, which publishes the Release
first and the documentation second.

### Staging — on every merge to `main`

1. Merge the pull request. `ci.yml` runs on the push to `main`.
2. The existing `docs-build` job builds the site (`npm install && npm run docs:build`), and
   two steps guarded by `github.event_name == 'push' && github.ref == 'refs/heads/main'` then
   run. A topic-branch push or a pull request publishes nothing.
3. `bash scripts/stamp-docs-deploy.sh staging` writes `version.json` into the build output and
   marks the build non-indexable (`robots.txt` disallow plus `X-Robots-Tag: noindex, nofollow`
   via `_headers`). Staging is publicly reachable; the repository and `main` are public
   already, so what is refused is indexing, not access.
4. `npm run docs:deploy:staging` publishes. A failed deploy fails the job — red CI, never a
   silent pass.

### Production — on a pushed `v*` tag

1. Follow `github-release.md` through the tag push. `release.yml` builds the four native
   tarballs and publishes the GitHub Release.
2. In parallel with those builds, the `docs-drift` job runs `cargo run -p xtask -- gen-docs
   --check` against the **tagged** tree. It exists because `ci.yml` never runs on a tag push
   (`on: push: branches: ["**"]` plus `pull_request` matches no tag), so this is the only place
   a tag's generated reference pages — `docs/language.md`, `docs/drivers.md`, `docs/server.md` —
   are checked against the binary being released alongside them.
3. `docs-deploy-production` runs only after **both** succeed (`needs: [release, docs-drift]`): a
   failed binary release never leaves qfs.qmu.co.jp advertising a version nobody can install, and
   a drifted reference page never reaches the hostname at all.
4. It builds the site from the **tagged** checkout, stamps it with
   `bash scripts/stamp-docs-deploy.sh production`, and runs `npm run docs:deploy:production`.
   The tracked `docs/public/robots.txt` ships unchanged, so production is indexable.

**A red `docs-drift`.** The GitHub Release still publishes — the binary is not at fault — and
`docs-deploy-production` is skipped, so qfs.qmu.co.jp keeps serving the previously published
version. Recover by running `cargo run -p xtask -- gen-docs` on `main`, committing the result,
and cutting a new tag; re-running the deploy job alone would republish the same drifted tree.

### By hand (recovery only)

From the repository root, with `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` in the
environment:

```sh
npm install
npm run docs:build
bash scripts/stamp-docs-deploy.sh staging      # or: production
npm run docs:deploy:staging                    # or: npm run docs:deploy:production
```

`npm run docs:deploy:dry-run` exercises both environments without publishing and needs no
credential.

### Credentials

Two GitHub Actions repository secrets, referenced by name and never by value — in the tree
there are only the names:

| Secret | What it is | Scope needed |
| --- | --- | --- |
| `CLOUDFLARE_API_TOKEN` | API token the deploy authenticates with | `Account → Workers Scripts → Edit` and `Account → Account Settings → Read`, on the one account. **No zone scope** — with no routes declared, a steady-state deploy never reads or writes the zone |
| `CLOUDFLARE_ACCOUNT_ID` | the account the two Workers live in | — |

Both are set on the deploying **job** rather than the workflow, so the Rust build jobs and the
release job keep seeing only the auto-provided `GITHUB_TOKEN`.

**Outstanding as of 2026-08-18 — the live secret is still the wide token.** The scope in the
table is what a deploy *needs* now that `docs/wrangler.toml` declares no routes; it is not yet
what `CLOUDFLARE_API_TOKEN` *holds*. The stored value is the original token, minted with
`Zone → Workers Routes → Edit` on `qmu.co.jp` back when the config declared the two custom
domains, and it still carries that scope. Narrowing it is three acts in the Cloudflare and GitHub
consoles that no change in this repository can perform: mint a replacement holding only the two
account permissions above, replace the `CLOUDFLARE_API_TOKEN` repository secret with it, and
revoke the wide one. Until then a leaked secret can still attach a Worker to any hostname in the
zone, the apex included — the exposure removing the routes was meant to end. The removal is what
made the narrowing safe to do, not the narrowing itself; tracked in
`.workaholic/tickets/todo/20260818124500-the-docs-deploy-token-cannot-be-narrowed-while-routes-are-declared.md`.

Attaching or re-attaching a custom domain is the one operation that still needs
`Zone → Workers Routes → Edit`. Do it from the dashboard (above) rather than widening the CI
secret; a token that can bind a Worker to a hostname can bind it to any hostname in the zone.

## Confirmation

Every deploy stamps the tree it published with `version.json`, so both hostnames answer
"which commit are you serving?" without an authenticated look at Cloudflare. These are the
same checks the two trigger tickets used.

**Staging, after a merge to `main`:**

```sh
curl -sS https://staging-qfs.qmu.co.jp/version.json
curl -sI https://staging-qfs.qmu.co.jp | head -1
curl -sS https://staging-qfs.qmu.co.jp/robots.txt
```

**Pass** when `version.json` reports `"environment": "staging"` and a `commit` equal to the
merge commit on `main`, the page returns `200`, and the response carries
`X-Robots-Tag: noindex, nofollow`. Also inspect the workflow run for that merge: the two publish
steps ran, and `Published <sha> to https://staging-qfs.qmu.co.jp` is in the job summary.

**The header is the check, not `robots.txt` — corrected 2026-08-19 against the live hostname.**
This pass condition used to read "`robots.txt` is the `Disallow: /` form", and the deployed file
does still end with that group. But Cloudflare **prepends** a Managed Content block to it, and that
block contains its own `User-agent: *` group ending in `Allow: /`. Under RFC 9309 a crawler merges
every group matching the same user-agent token and, where an allow and a disallow are equally
specific, takes the least restrictive — so `Allow: /` and `Disallow: /` merge to **allow**, and the
`robots.txt` half of the staging guard does not bind. `X-Robots-Tag: noindex, nofollow`, which
`stamp-docs-deploy.sh` sets on the staging environment only, is what actually keeps staging out of
an index, and it is present on the live hostname. Production correctly carries no such header.
Checking the `Disallow: /` line alone would therefore have passed while proving nothing.

**Production, after a `vX.Y.Z` tag:**

```sh
curl -sS https://qfs.qmu.co.jp/version.json
curl -sI https://qfs.qmu.co.jp | head -1
```

**Pass** when `version.json` reports `"environment": "production"` and a `ref` equal to the
tag, and the page returns `200`. Confirm the GitHub Release first (`github-release.md`'s
post-merge check) — the docs job runs after it, so a missing Release explains a missing docs
publish and the two are not diagnosed separately.

**Failure, either environment:** the job is red and the previously published version keeps
serving. Cloudflare has no partially-applied state to unwind; re-running the failed job, or the
by-hand path above, republishes the same commit.
