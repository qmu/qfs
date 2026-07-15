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
last_seen: 2026-07-15T16:35:34+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Slack workspace-namespace still advertises Verb::Rm with no query grammar

## Description

This branch fixed the file-node path-addressed delete, but the workspace `SlackNode::Files` namespace still advertises `Verb::Rm` (see [7c0763f](https://github.com/qmu/qfs/commit/7c0763f) in `packages/qfs/crates/driver-slack/src/lib.rs`), which `qfs run` has no grammar to invoke (only the interactive shell has `rm`). It is harmless (the `cp`/upload shorthands use it) but is a latent dead-capabilit… (`slack-workspace-namespace-still-advertises-verb.md`, origin `3dae249`)

## How to Fix

If a namespace-level bulk file delete is ever wanted from `qfs run`, either add a `remove /slack/<ws>/files where id == …` capability or drop the unused `Rm` from the namespace; otherwise leave it for the shell.

