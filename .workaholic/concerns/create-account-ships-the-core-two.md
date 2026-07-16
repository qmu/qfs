---
type: Concern
origin_pr: 
origin_pr_url: 
origin_branch: work-20260705-173620
origin_commit: 
created_at: 2026-07-06T00:00:00+09:00
last_seen: 2026-07-06T00:00:00+09:00
first_seen: 2026-07-06T00:00:00+09:00
concern_id: create-account-ships-the-core-two
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# CREATE ACCOUNT's SECRET reference form is unimplemented (no bind-time account credential resolution)

## Description

> **Rescoped 2026-07-15** by the missions/tickets reframing, per the `the-carried-create-account-ships-the`
> concern's recorded fix ("re-scope that concern's body to the `SECRET` edge alone, so its stale
> blocker note stops misleading readers"). That carried concern is now resolved and archived; this
> one stays `active` because the `SECRET` edge is genuinely untouched. The original body scoped out
> **two** edges — the second is retired, see below.

The in-language account surface (ticket 20260703040000) shipped the owner-approved core: `CREATE
ACCOUNT <provider> '<label>'` records consent (gated on a signed-in operator, sharing the CLI
`qfs account add` writer), `/sys/accounts` is a queryable selectors-only registry (no token column,
Google's driver trio collapsed to one `google` row), and `REMOVE /sys/accounts/<provider>/<label>`
deletes an account (token + consent). One edge from the ticket sketch remains deferred:

**The `SECRET '<ref>'` clause is not implemented.** The sketch showed `CREATE ACCOUNT github 'work'
SECRET 'vault:github/work'`. A service account resolves its credential from the vault (sealed
out-of-band); there is **no bind-time external-reference (`env:`/`vault:`) resolution for accounts**
today (unlike a mount's `CONNECT … SECRET`). Adding a parse-only clause would be a surface that
cannot resolve at bind — against "docs true / no fake success" — so it is omitted.

Verified still true against the **v0.0.71** binary on 2026-07-15: `create account github 'work'
secret 'vault:github/work'` returns `parse_error` / `UNEXPECTED_TOKEN`, and `create_account_stmt`
(`parser/src/grammar.rs:2364`) reads only provider + label + an optional `APP` clause.

### Retired edge (recorded, not silently dropped)

The original sub-item 2 — *"a Google account whose label is an email cannot be removed by a `REMOVE`
path"*, blocked on `EffectNode` carrying no filter — is **retired**. The effect-selector channel
shipped and `driver-sys` resolves the filter off it. Verified against v0.0.71 on 2026-07-15:
`remove /sys/accounts where account == '<an email>'` previews with `selector: ["account"]` and stops
only at the standard destructive-set-wide commit gate, not at a capability error. `rotate`/`revoke`
stay CLI-only by rule (they need a new secret value).

## How to Fix

**SECRET reference for accounts**: wire bind-time resolution of an account credential from an
`env:`/`vault:` reference (a new capability), then accept the `SECRET` clause on `CREATE ACCOUNT`
and store the reference where the cloud bind reads it.

This is now an acceptance item of the `declared-drivers-are-the-normal-way-to-add-a-service`
mission — it is the account half of the roadmap's 🧭 cloud-account-declaration gap, and the reason
it is a *mission* item rather than a lone fix is that the missing capability (bind-time reference
resolution for accounts) is the same one cloud account declarations need.
