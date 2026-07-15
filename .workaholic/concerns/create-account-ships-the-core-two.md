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
---

# CREATE ACCOUNT ships the core; two edges are scoped out (SECRET reference form, Google-email in-language REMOVE)

## Description

The in-language account surface (ticket 20260703040000) shipped the owner-approved core: `CREATE
ACCOUNT <provider> '<label>'` records consent (gated on a signed-in operator, sharing the CLI
`qfs account add` writer), `/sys/accounts` is a queryable selectors-only registry (no token column,
Google's driver trio collapsed to one `google` row), and `REMOVE /sys/accounts/<provider>/<label>`
deletes an account (token + consent). Two edges from the ticket sketch are deliberately deferred:

1. **The `SECRET '<ref>'` clause is not implemented.** The sketch showed `CREATE ACCOUNT github
   'work' SECRET 'vault:github/work'`. A service account resolves its credential from the vault
   (sealed out-of-band); there is **no bind-time external-reference (`env:`/`vault:`) resolution for
   accounts** today (unlike a mount's `CONNECT … SECRET`). Adding a parse-only clause would be a
   surface that cannot resolve at bind — against "docs true / no fake success" — so it is omitted.

2. **A Google account whose label is an email cannot be removed by a `REMOVE` path.** `@` is a path
   version coordinate (the lexer binds `a@qmu.jp` as segment `a` + version `qmu.jp`, and
   `render_path` drops the version), so an email cannot ride in `REMOVE /sys/accounts/google/<email>`
   — the applier would receive the truncated `…/a`. Path-safe cloud labels (`github/work`) remove
   cleanly; Google-email accounts use the CLI (`qfs account remove google <email>`). `rotate`/`revoke`
   stay CLI-only by rule (they need a new secret value).

## How to Fix

1. **SECRET reference for accounts**: wire bind-time resolution of an account credential from an
   `env:`/`vault:` reference (a new capability), then accept the `SECRET` clause on `CREATE ACCOUNT`
   and store the reference where the cloud bind reads it.
2. **Google-email REMOVE**: support a filter-based remove on `/sys/accounts` (`REMOVE /sys/accounts
   WHERE account == '<email>'`) — the email rides safely in a string literal. This needs the `/sys`
   applier to receive the `WHERE` predicate/matched rows (today `/sys` removes are path-addressed and
   `EffectNode` carries no filter), so it is a small evaluator/applier extension, not a one-liner.
