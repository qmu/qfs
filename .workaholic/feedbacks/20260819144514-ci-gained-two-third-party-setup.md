---
type: Feedback
title: CI gained two third-party setup actions
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-08-19T14:45:14+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: ci-gained-two-third-party-setup
owner: a@qmu.jp
mission: [the-current-situation-of-qfs-is-documented-as-it-actually-stands]
tickets: [20260817131540-file-the-bun-plgg-md-parse-defect-upstream.md]
origin_pr: 98
origin_pr_url: https://github.com/qmu/qfs/pull/98
origin_branch: work-20260818-224038
origin_commit: a678f8d
last_seen: 2026-08-19T14:45:14+09:00
---

# CI gained two third-party setup actions

## Description

`viewer-check-all` previously used only `actions/*`. Installing bun and deno adds

## How to Fix

Pin both to commit SHAs if the repository decides workflow actions should be
