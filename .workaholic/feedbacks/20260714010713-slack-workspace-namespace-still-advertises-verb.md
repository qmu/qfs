---
type: Feedback
title: Slack workspace-namespace still advertises Verb::Rm with no query grammar
kind: concern
source: development
created_at: 2026-07-14T01:07:13+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: slack-workspace-namespace-still-advertises-verb
owner: 
mission: 
tickets: [20260713234132-slack-file-detach-verb-mismatch.md, 20260713234133-gmail-attachment-id-not-exposed.md]
origin_pr: 39
origin_pr_url: https://github.com/qmu/qfs/pull/39
origin_branch: work-20260713-233938
origin_commit: 3dae249
last_seen: 2026-07-28T13:09:57+09:00
closed: superseded
---

# Slack workspace-namespace still advertises Verb::Rm with no query grammar

## Description

`driver-slack` has **zero diff** versus `origin/main`, so the Files namespace still advertises `Verb::Rm` with no grammar behind it, pinned by a test (see [3dae249](https://github.com/qmu/qfs/commit/3dae249)). The compiled driver was deliberately not deleted, so the advertisement survives the branch. This branch's lexer fix addresses a *different* advertisement lie.

## How to Fix

Drop the namespace-level advertisement, or give it a grammar — ideally before the successor mission measures a twin against this surface.
