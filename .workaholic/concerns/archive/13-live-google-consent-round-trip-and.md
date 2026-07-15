---
type: Concern
origin_pr: 13
origin_pr_url: https://github.com/qmu/qfs/pull/13
origin_branch: work-20260703-022500
origin_commit: 92a5e30
created_at: 2026-07-03T05:41:25+09:00
last_seen: 2026-07-03T05:41:25+09:00
first_seen: 2026-07-03T05:41:25+09:00
concern_id: live-google-consent-round-trip-and
severity: moderate
status: resolved
resolved_by_pr: 96c936a
resolved_by_commit: 
---

# Live Google consent round-trip and /drive read verification pending

## Description

The live Google round-trip and the `/drive` read on the union-scope token still need owner-attended verification with a real browser (see [604321c](https://github.com/qmu/qfs/commit/604321c) in `packages/qfs/crates/google-auth/src/authorize.rs` and `packages/qfs/crates/qfs/src/account.rs`).

## How to Fix

Ship as v0.0.15 so the owner redoes `account add google` on the fixed binary with the actual Google OAuth flow and verifies that a subsequent `/drive` read returns real rows from the union-scope refresh token.
