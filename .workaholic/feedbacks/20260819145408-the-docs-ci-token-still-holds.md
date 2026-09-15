---
type: Feedback
title: The docs CI token still holds zone-wide Workers Routes
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-08-19T14:54:08+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: the-docs-ci-token-still-holds
owner: a@qmu.jp
mission: [the-documentation-site-publishes-itself-staging-on-merge-production-on-release]
tickets: []
origin_pr: 99
origin_pr_url: https://github.com/qmu/qfs/pull/99
origin_branch: work-20260818-224556
origin_commit: 56b25c3
last_seen: 2026-08-19T14:54:08+09:00
---

# The docs CI token still holds zone-wide Workers Routes

## Description

`CLOUDFLARE_API_TOKEN` is still the token minted with `Zone → Workers Routes:

## How to Fix

Mint a replacement holding only `Account → Workers Scripts: Edit` and
