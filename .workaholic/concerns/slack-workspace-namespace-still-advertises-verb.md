---
type: Concern
concern_id: slack-workspace-namespace-still-advertises-verb
mission: 
tickets: [20260713234132-slack-file-detach-verb-mismatch.md, 20260713234133-gmail-attachment-id-not-exposed.md]
origin_pr: 39
origin_pr_url: https://github.com/qmu/qfs/pull/39
origin_branch: work-20260713-233938
origin_commit: 3dae249
created_at: 2026-07-14T01:07:13+09:00
first_seen: 2026-07-14T01:07:13+09:00
last_seen: 2026-07-28T12:51:29+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Slack workspace-namespace still advertises Verb::Rm with no query grammar

## Description

`SlackNode::Files` still advertises `Verb::Rm`, while the taught detach form resolves to `SlackNode::File` and `Verb::Remove` — a different node and verb — so the namespace-level `Rm` remains grammar-less (see [3dae249](https://github.com/qmu/qfs/commit/3dae249)).

## How to Fix

Drop the namespace-level `Verb::Rm` advertisement, or give it a grammar.

