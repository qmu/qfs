---
created_at: 2026-08-18T12:45:00+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [the first v0.0.111 tag — the production Worker and its custom domain must exist first]
mission: [the-documentation-site-publishes-itself-staging-on-merge-production-on-release]
merge_policy: review
verification_handoff:
---

# The docs-deploy CI token cannot be narrowed while `routes` are declared

## Overview

Found while sizing the Cloudflare API token for the two new repository secrets, at the ship of
PR #74. The question asked was whether the zone-scoped permissions are only needed for the
initial setup. They are not, as `docs/wrangler.toml` stands.

## What was measured

Against **wrangler 4.123.0** — the version `package.json` pins — reading its shipped
`wrangler-dist/cli.js`:

- `triggersDeploy` splits configured routes into `routesOnly` and `customDomainsOnly`, then
  calls `publishCustomDomains(...)` whenever `customDomainsOnly.length > 0`. There is **no**
  "already attached, skip" branch: it runs on every deploy.
- `publishCustomDomains` branches on `!process.stdout.isTTY`. CI is non-TTY, so it skips the
  `POST …/domains/changeset?replace_state=true` diff entirely and sets
  `override_existing_origin = true` and `override_existing_dns_record = true`, then issues:

  ```
  PUT /accounts/{id}/workers/scripts/{name}/domains/records
  ```

- Separately, the block guarded by `!wantWorkersDev && workersDevInSync && routes.length !== 0`
  calls `getZoneForRoute` and `fetchListResult('/zones/{zone}/workers/routes')` on every deploy
  to check for a Worker already bound to the pattern.

So a steady-state `wrangler deploy --env staging` needs zone-scoped read **and** write for as
long as the route stays in the config.

## Why it is worth changing

`Workers Routes: Edit` cannot be scoped below a whole zone. As configured, a leaked
`CLOUDFLARE_API_TOKEN` from GitHub Actions can attach a Worker to **any** hostname on
`qmu.co.jp` — the apex included — not merely the two documentation hostnames. Narrowing the
token to `Workers Scripts: Edit` bounds a leak to "the docs site's content can be replaced",
which is the irreducible permission of a docs-publishing job.

## Implementation

1. Delete the `routes = [...]` line from `[env.staging]` and `[env.production]` in
   `docs/wrangler.toml`, replacing each with a comment naming the hostname that environment
   serves and stating that the custom domain is attached out of band and deliberately not
   re-asserted on every deploy.
2. Mint a replacement API token holding only **Account → Workers Scripts: Edit** and
   **Account → Account Settings: Read**, scoped to the one account. Replace the
   `CLOUDFLARE_API_TOKEN` repository secret with it and revoke the wide token.
3. Update `.workaholic/deployments/docs-site.md`: the hostnames are now attached manually and
   the config no longer declares them, and the by-hand recovery path needs a token with the
   wide scopes to re-attach a domain.
4. Confirm a normal merge still publishes: `curl -sS https://staging-qfs.qmu.co.jp/version.json`
   reports the new merge commit.

## Considerations

- **`workers_dev = false` must stay explicit.** `getSubdomainValues` defaults `workers_dev` to
  `routes.length === 0`, so removing the routes without the explicit `false` would re-enable a
  `workers.dev` hostname for both environments.
- **Nothing detaches the existing domains.** The destructive `PUT …/routes` runs only when
  `routesOnly.length > 0`, and `publishCustomDomains` only when `customDomainsOnly.length > 0`.
  Both are empty after step 1.
- **The cost is declarativeness.** The tree stops stating which hostname each environment
  serves in an executable form, and a deleted-and-recreated Worker will not get its domain back
  automatically. The comment plus the deployment record carry it as prose instead. This is the
  trade being accepted, not an oversight.
- **Blocked until the production pair exists.** The first `v0.0.111` tag is what creates
  `qfs-docs-production` and attaches `qfs.qmu.co.jp`; that attach needs the wide token. Do not
  narrow before it has run.
- **Re-verify on a wrangler major bump.** The behaviour above is read from 4.123.0's shipped
  bundle, not from a documented contract.

## Key Files

- `docs/wrangler.toml`
- `.workaholic/deployments/docs-site.md`
- `.github/workflows/ci.yml`, `.github/workflows/release.yml` (secret consumers; unchanged)

## Blocked — 2026-08-18 (work-20260818-194054)

**Blocker: minting a Cloudflare API token, replacing a GitHub Actions repository secret and
revoking the old token are console acts outside this repository, and this runner holds neither
credential.** The repository-side work is done and verified; step 2 is all that is left, and it
is not a change to any file.

### What is already landed

Steps 1 and 3 shipped ahead of this ticket, in commit `db0a1ee` ("Stop declaring the docs custom
domains", PR #85, merged 2026-08-18) — the same finding, driven under the mission before this
ticket was reached:

- `docs/wrangler.toml` declares no `routes` in either environment; both env blocks keep an
  explicit `workers_dev = false`, which is load-bearing once the routes are gone.
- `.workaholic/deployments/docs-site.md` records that the hostnames are attached out of band and
  that re-attaching one needs the wide scope the CI secret is meant to shed.

### Step 4, verified

The `curl` in the Implementation section is unreachable from this container — outbound HTTPS to
both hostnames is refused by the proxy (`curl: (56) CONNECT tunnel failed, response 403`), so the
deployment record's other stated check was used instead: inspect the workflow run for the merge.

`ci.yml` run [32177242086](https://github.com/qmu/qfs/actions/runs/32177242086), the push of
`c17cdf1` — the merge commit that carries the routes removal — completed `success`, and inside it
the `docs site production build` job ran **Stamp the staging build with its commit** and
**Publish to staging-qfs.qmu.co.jp**, both `success`, finishing 2026-08-18T19:33:38Z. A normal
merge therefore still publishes with no routes declared, which is what step 4 asks. The
production leg is likewise proven: `release.yml` run
[32141202957](https://github.com/qmu/qfs/actions/runs/32141202957) for `v0.0.111` published
`qfs.qmu.co.jp` successfully at 13:21:27Z, so the production Worker and its custom domain exist
and this ticket's `depends_on` is satisfied.

### Why step 2 cannot be done here

Two checked facts, not a forecast:

1. **No Cloudflare credential exists in this environment.** `env | grep -ci cloudflare` → `0`, and
   the repository root carries only `.env.example`. There is nothing to authenticate a
   token-minting or token-revoking API call with.
2. **This runner cannot write a GitHub Actions repository secret.** No secret-writing tool is
   exposed to it, and writes through this session's GitHub proxy are refused in general — observed
   verbatim on an in-scope repository during the previous unit: `{"message":"Write access to this
   GitHub API path is not permitted through this proxy."}` (HTTP 403).

### What unblocks it

A person with the Cloudflare account and the repository's secret settings: mint a token holding
only **Account → Workers Scripts: Edit** and **Account → Account Settings: Read**, replace
`CLOUDFLARE_API_TOKEN` with it, revoke the wide token, then let one merge to `main` publish to
confirm. The deployment record now states this as outstanding where an operator reads it, so the
narrow scope in its Credentials table is not mistaken for the scope the live secret has.

## Still blocked — 2026-08-18 22:4x UTC (work-20260818-224556)

Re-checked by a later unattended `/implement` run. Step 2 is still the only thing left and is still
a set of console acts this runner cannot perform; what did move is the evidence under step 4, which
now rests on a merge two hours newer than the one the entry above cites.

What was actually run this time:

| Check | Command / source | Result |
| --- | --- | --- |
| Is a Cloudflare credential present now? | `env \| grep -ci cloudflare` | `0`; the repository root still carries only `.env.example` — nothing to mint or revoke a token with |
| Can the published hostnames be read directly? | `curl -sS https://staging-qfs.qmu.co.jp/version.json`, same for `qfs.qmu.co.jp` | `curl: (56) CONNECT tunnel failed, response 403` on both — outbound HTTPS to them is still refused by this container's proxy, so the workflow-run check stays the substitute |
| Does a routeless merge still publish? | `ci.yml` run [32191206355](https://github.com/qmu/qfs/actions/runs/32191206355), the push of `94bb6e7` (merge of #97) | job **docs site production build** `success`; its steps **Stamp the staging build with its commit** and **Publish to staging-qfs.qmu.co.jp** both `success`, finishing 2026-08-18T22:07:48Z |

So the routes removal is not merely "was fine once": every merge since has published, the newest
being two hours old at the time of this check. The exposure the ticket exists to close is unchanged
— `CLOUDFLARE_API_TOKEN` still holds `Zone → Workers Routes → Edit` on `qmu.co.jp` until a person
mints the narrow replacement, swaps the repository secret and revokes the wide one.
