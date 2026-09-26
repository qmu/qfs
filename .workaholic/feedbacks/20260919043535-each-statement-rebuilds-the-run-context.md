---
type: Feedback
title: Each statement rebuilds the run context
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-09-19T04:35:35+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: each-statement-rebuilds-the-run-context
owner: 
mission: []
tickets: [20260919001500-install-a-shipped-declaration-from-the-binary-that-carries-it.md]
origin_pr: 135
origin_pr_url: https://github.com/qmu/qfs/pull/135
origin_branch: work-20260919-000500
origin_commit: 4e0d4e9
last_seen: 2026-09-19T04:35:35+09:00
---

# Each statement rebuilds the run context

## Description

A 27-statement install builds the Engine and ReadRegistry 27 times, because it reuses `dispatch_run` per statement (see [bfc96ad](https://github.com/qmu/qfs/commit/bfc96ad) in `packages/qfs/crates/cmd/src/lib.rs`)

## How to Fix

Hoist the context if a declaration ever grows enough for it to matter; reusing the tested path was worth the cost at this size
