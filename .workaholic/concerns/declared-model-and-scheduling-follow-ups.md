---
type: Concern
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: [20260711121526-chatwork-declared-driver-with-file-handling.md, 20260711121534-oauth-style-declared-driver-rewrite.md, 20260711121535-server-scheduling-semantics-revisit.md, 20260711121528-reply-with-attachment-cross-service.md]
origin_pr: 33
origin_pr_url: https://github.com/qmu/qfs/pull/33
origin_branch: work-20260711-121525
origin_commit: f1a3d21
created_at: 2026-07-12T01:52:23+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-12T01:52:23+09:00
concern_id: declared-model-and-scheduling-follow-ups
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Declared-model and scheduling follow-ups

## Description

Partially resolved by PR #35 (v0.0.59): sub-item (1)'s two generic declared-evaluator primitives (`FOLLOW <field>` for the cross-host download URL, `ENCODE multipart` for upload) landed, and sub-item (3)'s daemon real-clock sweeper + `/server/jobs/<name>/runs` read-back collection landed in `crates/qfs/src/sweeper.rs`. Still remaining: (1-live) verify live that Chatwork message POST tolerates the… (`declared-model-and-scheduling-follow-ups.md`, origin `f1a3d21`)

## How to Fix

Each remainder is a scoped follow-up ticket when prioritized: live-verify the Chatwork encoding (rounds 3/10 of the owner-attended live rounds); plumb the OAuth app into the declared-secrets adapter; extend the reply surfaces for Slack threading.

