---
type: Feedback
title: The upstream build wart is real and stays open
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-08-19T14:45:14+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: the-upstream-build-wart-is-real
owner: a@qmu.jp
mission: [the-current-situation-of-qfs-is-documented-as-it-actually-stands]
tickets: [20260817131540-file-the-bun-plgg-md-parse-defect-upstream.md]
origin_pr: 98
origin_pr_url: https://github.com/qmu/qfs/pull/98
origin_branch: work-20260818-224038
origin_commit: a678f8d
last_seen: 2026-08-19T14:45:14+09:00
---

# The upstream build wart is real and stays open

## Description

`plgg-md`'s bundler still emits the control-character class endpoints as raw bytes

## How to Fix

[qmu/plgg#131](https://github.com/qmu/plgg/issues/131) is open and carries the ask.
