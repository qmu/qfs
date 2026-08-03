---
type: Feedback
title: Commit-side apply registry still binds quietly
kind: concern
source: development
created_at: 2026-07-03T14:22:57+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: commit-side-apply-registry-still-binds
owner: 
mission: 
tickets: []
origin_pr: 15
origin_pr_url: https://github.com/qmu/qfs/pull/15
origin_branch: work-20260703-063000
origin_commit: 94dea14
last_seen: 2026-07-03T14:22:57+09:00
closed: resolved
resolved_by_commit: 3d43e99
---

# Commit-side apply registry still binds quietly

## Description

The scan-time unlock covers READS; the commit-side apply registry still opens the store only through the quiet paths, so a terminal `--commit` against a cloud mount without `QFS_PASSPHRASE` can still fail its bind silently (see [96c936a](https://github.com/qmu/qfs/commit/96c936a) in `packages/qfs/crates/qfs/src/shell.rs`).

## How to Fix

Apply the same lazy, prompt-at-proven-need treatment to `commit.rs`'s cloud apply drivers.
