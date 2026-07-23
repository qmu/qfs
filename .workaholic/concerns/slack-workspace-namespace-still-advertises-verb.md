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
last_seen: 2026-07-24T01:02:01+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Slack workspace-namespace still advertises Verb::Rm with no query grammar

## Description

The Slack Files namespace still advertises the grammar-less Verb::Rm; driver-slack was not touched on this branch

## How to Fix

Add query grammar for the Slack Files Verb::Rm or stop advertising it

