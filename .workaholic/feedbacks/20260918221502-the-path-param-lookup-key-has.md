---
type: Feedback
title: The `path.<param>` lookup key has no shipped consumer
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-09-18T22:15:02+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: the-path-param-lookup-key-has
owner: 
mission: []
tickets: [20260918040200-say-which-credential-resolution-failed-instead-of-http-auth.md, 20260918040300-ship-the-private-channels-view-so-a-channel-miss-is-evidence.md, 20260918041500-upload-a-file-to-slack-through-a-scoped-three-call-primitive.md]
origin_pr: 132
origin_pr_url: https://github.com/qmu/qfs/pull/132
origin_branch: work-20260918-040100
origin_commit: cfad29f
last_seen: 2026-09-18T22:15:02+09:00
---

# The `path.<param>` lookup key has no shipped consumer

## Description

The declared-lookup widening is correct and tested, but the upload map that motivated it now passes the channel through instead (see [c07fd70](https://github.com/qmu/qfs/commit/c07fd70) in `packages/qfs/crates/exec/src/declared.rs`)

## How to Fix

Either consume it from the reference-resolution ticket above, or remove it if that ticket lands another way
