---
type: Feedback
title: A Slack channel resolves by id only, on every path
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-09-18T22:15:02+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: a-slack-channel-resolves-by-id
owner: 
mission: []
tickets: [20260918040200-say-which-credential-resolution-failed-instead-of-http-auth.md, 20260918040300-ship-the-private-channels-view-so-a-channel-miss-is-evidence.md, 20260918041500-upload-a-file-to-slack-through-a-scoped-three-call-primitive.md]
origin_pr: 132
origin_pr_url: https://github.com/qmu/qfs/pull/132
origin_branch: work-20260918-040100
origin_commit: cfad29f
last_seen: 2026-09-18T22:15:02+09:00
---

# A Slack channel resolves by id only, on every path

## Description

Reads, posts and now uploads all substitute the path segment into a Slack method that takes an id, so a documented name-addressed example cannot work (see [b062f17](https://github.com/qmu/qfs/commit/b062f17) in `packages/qfs/crates/skill/assets/examples/slack_driver.qfs`)

## How to Fix

Land ticket `20260918044700`, which fixes reference resolution once for the whole surface rather than per map
