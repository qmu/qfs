---
type: Feedback
title: A declared driver has no upgrade path, so a shipped declaration fix does not reach a live mount
kind: concern
source: development
created_at: 2026-08-04T20:50:42+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: a-declared-driver-has-no-upgrade
owner: 
mission: []
tickets: [20260801061500-chatwork-messages-view-returns-unread-only.md]
origin_pr: 32
origin_pr_url: https://github.com/qmu/qfs/pull/32
origin_branch: work-20260803-221340
origin_commit: e81e5d6
last_seen: 2026-08-04T20:50:42+09:00
---

# A declared driver has no upgrade path, so a shipped declaration fix does not reach a live mount

## Description

The fix lives in a shipped asset, but an operator who already ran the install

## How to Fix

Give installed declarations a version or content hash, and a way to ask whether a
