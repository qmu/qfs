---
type: Concern
concern_id: the-type-catalog-and-the-type
mission: 
tickets: [20260713195008-effect-selector-channel-folder-rename.md, 20260714120000-effect-selector-uniform-migration.md, 20260714154144-general-of-type-assertion.md, 20260714182710-shell-face-slice1-ls-cat-describe-typed.md, 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md, 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md, 20260714182740-shell-face-type-mount-and-describe-builtin.md, 20260714220213-resume-shell-face-slices-and-report.md]
origin_pr: 41
origin_pr_url: https://github.com/qmu/qfs/pull/41
origin_branch: work-20260714-111817
origin_commit: 7752cb3
created_at: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-15T16:35:34+09:00
last_seen: 2026-07-28T12:51:29+09:00
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# The `/type` catalog and the type resolver translate the stored key differently

## Description

The path-form vs reference-name divergence for `sys_drivers kind='type'` rows still stands as an unwritten encoding rule (see [7752cb3](https://github.com/qmu/qfs/commit/7752cb3) in `crates/qfs/src/type_catalog.rs`).

## How to Fix

Write the encoding rule down and unify it across catalog and resolver.

