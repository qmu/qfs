---
type: Feedback
title: The declared write path carries bytes as a JSON array
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-09-18T22:15:02+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: the-declared-write-path-carries-bytes
owner: 
mission: []
tickets: [20260918040200-say-which-credential-resolution-failed-instead-of-http-auth.md, 20260918040300-ship-the-private-channels-view-so-a-channel-miss-is-evidence.md, 20260918041500-upload-a-file-to-slack-through-a-scoped-three-call-primitive.md]
origin_pr: 132
origin_pr_url: https://github.com/qmu/qfs/pull/132
origin_branch: work-20260918-040100
origin_commit: cfad29f
last_seen: 2026-09-18T22:15:02+09:00
---

# The declared write path carries bytes as a JSON array

## Description

`value_to_json` renders `Value::Bytes` as an array of numbers, so an upload costs roughly four times the file's size in memory in flight (see [c07fd70](https://github.com/qmu/qfs/commit/c07fd70) in `packages/qfs/crates/codec/src/convert.rs`)

## How to Fix

Add an `ENCODE bytes` wire encoding that posts a `Value::Bytes` body verbatim as `application/octet-stream`; the upload primitive consumes it unchanged
