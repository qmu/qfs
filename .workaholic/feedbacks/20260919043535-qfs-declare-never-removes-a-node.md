---
type: Feedback
title: `qfs declare` never removes a node
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-09-19T04:35:35+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: qfs-declare-never-removes-a-node
owner: 
mission: []
tickets: [20260919001500-install-a-shipped-declaration-from-the-binary-that-carries-it.md]
origin_pr: 135
origin_pr_url: https://github.com/qmu/qfs/pull/135
origin_branch: work-20260919-000500
origin_commit: 4e0d4e9
last_seen: 2026-09-19T04:35:35+09:00
---

# `qfs declare` never removes a node

## Description

The command adds and replaces; locally-added nodes the binary does not ship are left alone, so such a declaration still reads `stale` after a successful install (see [bfc96ad](https://github.com/qmu/qfs/commit/bfc96ad) in `packages/qfs/crates/cmd/src/lib.rs`)

## How to Fix

Nothing, unless a `--prune` is wanted later; the output says so explicitly and `REMOVE VIEW|MAP|TYPE` remains the way to drop one
