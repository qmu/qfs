---
type: Feedback
title: Golden/anti-drift crate not exercised by per-crate runs
kind: concern
source: development
created_at: 2026-07-24T00:48:25+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: golden-anti-drift-crate-not-exercised
owner: a@qmu.jp
mission: [a-file-collection-is-a-declared-set-over-any-blob-source]
tickets: [20260722090100-design-brief-codec-relation-surface-and-13b-ruling.md, 20260722090200-per-row-decode-over-collected-sets.md, 20260722090300-documents-links-as-declared-registrations.md, 20260722090400-retire-the-compiled-markdown-driver.md, 20260722090500-cookbook-collection-recipes-execution-checked.md, 20260723100000-wire-read-by-path-mount-for-registered-views.md]
origin_pr: 22
origin_pr_url: https://github.com/qmu/qfs/pull/22
origin_branch: work-20260722-084645
origin_commit: 8bc902d
last_seen: 2026-07-28T13:09:57+09:00
---

# Golden/anti-drift crate not exercised by per-crate runs

## Description

Nothing here changes the test topology; the full-workspace ship gate is a process compensation, not a structural fix (see [8bc902d](https://github.com/qmu/qfs/commit/8bc902d)).

## How to Fix

Make the golden crate part of any per-crate run's scope.
