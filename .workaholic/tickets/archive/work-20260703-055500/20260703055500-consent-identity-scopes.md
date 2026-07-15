---
created_at: 2026-07-03T05:55:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 0.5h
commit_hash: 435eb6a
category: Changed
depends_on: []
---

# Consent scope union must include the OIDC identity pair (openid email)

Live-consent failure on released v0.0.15 (owner, first real paste-back round-trip): `state`
verification and the code exchange succeeded, but the flow died at the last step with
`profile lookup failed: userinfo status 401`. The granted scopes (visible in the pasted
redirect's `scope=` param) were exactly the four API scopes — gmail.modify, gmail.compose,
drive, analytics.readonly. `authorize` keys the account by the **userinfo profile email**, and
Google's OIDC userinfo endpoint rejects an access token that carries only API scopes. A latent
gap since the original loopback flow (never live-run: consent was behind the retired
`QFS_GOOGLE_CONSENT` opt-in and live verification used the stdin token import).

## Fix

Add `openid` and `email` to `all_google_scopes()` (`crates/qfs/src/google.rs`) so the minted
access token can call userinfo; keep the union otherwise least-privilege (no `profile` scope).
`include_granted_scopes=true` accumulates onto the owner's existing grant, so re-running
`qfs account add google` after the fix completes the onboarding.

## Quality Gate

- Scope-union unit test asserts the identity pair is present and the union is exactly the four
  API scopes + openid/email, with no broad `https://mail.google.com/` or `profile` grant.
- Workspace tests / clippy / fmt / gen-docs / gen-skills green; owner re-runs the live consent
  on the released v0.0.16 (the acceptance proof).
