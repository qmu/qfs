---
type: Feedback
title: The shipped-asset install-splitter is still copy-pasted across tests
kind: concern
source: development
created_at: 2026-08-04T20:50:42+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: the-shipped-asset-install-splitter-is
owner: 
mission: []
tickets: [20260801061500-chatwork-messages-view-returns-unread-only.md]
origin_pr: 32
origin_pr_url: https://github.com/qmu/qfs/pull/32
origin_branch: work-20260803-221340
origin_commit: e81e5d6
last_seen: 2026-08-04T20:50:42+09:00
---

# The shipped-asset install-splitter is still copy-pasted across tests

## Description

Three tests in `declared_driver.rs` each carried their own inline copy of the

## How to Fix

Fold the remaining two tests onto the shared helper. Mechanical, and nobody has
