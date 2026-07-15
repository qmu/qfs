---
type: Concern
mission:
tickets: [20260711010500-docs-slack-user-token-posting-guide.md, 20260711121528-reply-with-attachment-cross-service.md, 20260711121534-oauth-style-declared-driver-rewrite.md, 20260711121535-server-scheduling-semantics-revisit.md, 20260711121530-pdf-extraction-to-drive-pipeline.md, 20260711121531-transform-chain-composition.md]
origin_pr: 33
origin_pr_url: https://github.com/qmu/qfs/pull/33
origin_branch: work-20260711-121525
origin_commit: f1a3d21
created_at: 2026-07-12T01:52:23+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-12T01:52:23+09:00
concern_id: remaining-owner-attended-live-rounds
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Remaining owner-attended live rounds

## Description

The switch live round ran and passed (real Anthropic model, real Drive and Gmail-drafts writes, per-arm counts verified by read-back), but six live rounds remain owner-attended pending: the Slack user-token post (`/slack-me` preview+commit), a real Gmail cross-service reply into a live thread, a declared `/ghdecl` GitHub read via `AUTH ACCOUNT 'github'` (the live GitHub API needs a `User-Agent` he… (`remaining-owner-attended-live-rounds.md`, origin `f1a3d21`)

## How to Fix

Owner runs each round in a dedicated session and records evidence on the archived tickets. The archived resume ticket carries the worked operating pattern: statement files in the session scratchpad, PREVIEW and read-back by the assistant, every COMMIT triggered from the owner's real terminal, model key via `secret 'env:ANTHROPIC_API_KEY'`.

