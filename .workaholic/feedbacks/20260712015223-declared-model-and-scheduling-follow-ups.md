---
type: Feedback
title: Declared-model and scheduling follow-ups
kind: concern
source: development
created_at: 2026-07-12T01:52:23+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: declared-model-and-scheduling-follow-ups
owner: 
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: [20260711121526-chatwork-declared-driver-with-file-handling.md, 20260711121534-oauth-style-declared-driver-rewrite.md, 20260711121535-server-scheduling-semantics-revisit.md, 20260711121528-reply-with-attachment-cross-service.md]
origin_pr: 33
origin_pr_url: https://github.com/qmu/qfs/pull/33
origin_branch: work-20260711-121525
origin_commit: f1a3d21
last_seen: 2026-07-28T13:09:57+09:00
---

# Declared-model and scheduling follow-ups

## Description

Live Chatwork encoding verification, OAuth-app plumbing and Slack threading are untouched; `slack_driver.qfs` declares no thread-reply surface (see [f1a3d21](https://github.com/qmu/qfs/commit/f1a3d21)).

## How to Fix

Execute the follow-up missions for the declared model and scheduling.
