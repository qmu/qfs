---
type: Concern
concern_id: golden-anti-drift-crate-not-exercised
mission: [a-file-collection-is-a-declared-set-over-any-blob-source]
owner: a@qmu.jp
tickets: [20260722090100-design-brief-codec-relation-surface-and-13b-ruling.md, 20260722090200-per-row-decode-over-collected-sets.md, 20260722090300-documents-links-as-declared-registrations.md, 20260722090400-retire-the-compiled-markdown-driver.md, 20260722090500-cookbook-collection-recipes-execution-checked.md, 20260723100000-wire-read-by-path-mount-for-registered-views.md]
origin_pr: 22
origin_pr_url: https://github.com/qmu/qfs/pull/22
origin_branch: work-20260722-084645
origin_commit: 8bc902d
created_at: 2026-07-24T00:48:25+09:00
first_seen: 2026-07-24T00:48:25+09:00
last_seen: 2026-07-28T12:55:39+09:00
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Golden/anti-drift crate not exercised by per-crate runs

## Description

Nothing here changes the test-runner topology or adds a golden-crate trigger to per-crate runs (see [8bc902d](https://github.com/qmu/qfs/commit/8bc902d)).

## How to Fix

Make the golden crate part of any per-crate run's scope, or forbid per-crate substitution at the gate.

