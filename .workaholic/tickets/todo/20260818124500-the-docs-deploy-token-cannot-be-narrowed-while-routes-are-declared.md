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
