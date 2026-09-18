---
type: Feedback
title: A rowless write that needs a row now fails where it used to be silent
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-09-19T05:43:00+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: a-rowless-write-that-needs-a
owner: 
mission: []
tickets: [20260919050000-a-declared-write-addressed-at-its-target-reports-success-and-sends-nothing.md]
origin_pr: 137
origin_pr_url: https://github.com/qmu/qfs/pull/137
origin_branch: work-20260919-050000
origin_commit: 6eb17a9
last_seen: 2026-09-19T05:43:00+09:00
---

# A rowless write that needs a row now fails where it used to be silent

## Description

A declaration whose body reads `row.<field>` under an address-only write is now a structured refusal (see [fadcca7](https://github.com/qmu/qfs/commit/fadcca7) in `packages/qfs/crates/exec/src/declared.rs`)

## How to Fix

Nothing — the old behaviour sent nothing and said it had; a refusal is strictly more truthful. Listed so the change in behaviour is on the record.
