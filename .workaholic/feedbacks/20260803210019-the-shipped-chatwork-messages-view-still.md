---
type: Feedback
title: The shipped Chatwork messages view still reads unread-only
kind: concern
source: development
created_at: 2026-08-03T21:00:19+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: the-shipped-chatwork-messages-view-still
owner: a@qmu.jp
mission: [a-declared-write-resolves-a-name-the-way-a-query-does]
tickets: [20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md]
origin_pr: 30
origin_pr_url: https://github.com/qmu/qfs/pull/30
origin_branch: work-20260801-044839
origin_commit: cd4a3f0
last_seen: 2026-08-03T21:00:19+09:00
---

# The shipped Chatwork messages view still reads unread-only

## Description

The declared view calls `GET /v2/rooms/{room}/messages` without `force`, whose

## How to Fix

Rule between the three shapes written up in
