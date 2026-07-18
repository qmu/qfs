---
created_at: 2026-07-18T20:33:28+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: []
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# AUTH ACCOUNT declared drivers resolve OAuth-refreshed bearers via the mount's app

## Overview

`AccountBearerSecrets` (`packages/qfs/crates/qfs/src/declared_driver.rs:768-820`) hands back the raw
vault row at `(provider, account)`. That is correct for static-bearer providers — github, slack,
chatwork, cf (comment at `declared_driver.rs:720-724`) — but **wrong** for an OAuth provider whose
stored credential is a refresh token that must be exchanged for a live bearer before use. Handing
back the refresh token means the declared adapter returns a credential the request cannot
authenticate with.

Compounding this, `declared_mounts()` (`declared_driver.rs:464-491`) silently drops the
`path_binding` `app` column, so a mount's OAuth-app label never reaches the adapter — the exchange
has no app config to work from even if the arm existed.

Fix both:

- Carry `app` onto `DeclaredMount` (`declared_driver.rs:74-81`) and stop dropping it in
  `declared_mounts()`.
- Thread `app` into `declared_secrets` (`declared_driver.rs:711`).
- Grow an OAuth arm that composes the existing app-config plus the refreshing token source —
  `google_app_config` (`packages/qfs/crates/…/google.rs:79`), `google_stack_for_account`
  (`google.rs:205`) — falling back to the consent row's app via
  `packages/qfs/crates/qfs/src/secret_store.rs:580` `db_get_consent_app`. The declared adapter then
  returns a **live bearer**, not a refresh token.
- Static-bearer providers are unchanged (pass-through preserved); the adapter stays read-only and
  declarations carry only selectors, never tokens.

## Policies

- implementation/honest-surfaces — no silent unauthenticated call; if the app is missing the adapter
  fails closed with a structured, secret-free cause naming the missing app, rather than returning a
  token that cannot authenticate.
- design/data-sovereignty — tokens stay vaulted; declarations carry selectors only, and the OAuth
  exchange happens inside the adapter, not in the declaration.

## Quality Gate

1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo fmt --all --check`
4. Acceptance: `DeclaredMount` carries `app` (no longer dropped in `declared_mounts()`).
5. Acceptance: an OAuth declared driver resolves a **refreshed live bearer** via the mount's /
   consent row's app.
6. Acceptance: a missing app fails closed with a structured, secret-free app-naming error.
7. Acceptance: static-bearer providers (github/slack/chatwork/cf) are unchanged
   (regression-tested).
8. Acceptance: the adapter stays read-only; declarations carry only selectors, never tokens.
9. Verification: hermetic unit tests over the adapter with `InMemoryStore` vaults — static-bearer
   pass-through unchanged; the OAuth arm exercised against the google-auth crate's mock
   token-exchange doubles (`packages/qfs/crates/google-auth/src/tests.rs`); a `declared_mounts` test
   asserting `app` propagates.
10. Live Chatwork / Slack remainders handed to the owner-attended live backlog (recorded, not
    attempted).

## Considerations

- The OAuth arm only needs to fire for providers whose stored credential is a refresh token; keep
  the static-bearer branch as the default so existing providers take the unchanged fast path.
- The `app` fallback ordering (mount app → consent row app via `db_get_consent_app`) should be
  explicit so a mount that omits `app` still resolves when the consent row names one.
