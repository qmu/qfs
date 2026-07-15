---
type: Concern
concern_id: the-type-catalog-and-the-type
mission: language-design-review-layering-principles-and-semantic-gaps
tickets: [20260713195008-effect-selector-channel-folder-rename.md, 20260714120000-effect-selector-uniform-migration.md, 20260714154144-general-of-type-assertion.md, 20260714182710-shell-face-slice1-ls-cat-describe-typed.md, 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md, 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md, 20260714182740-shell-face-type-mount-and-describe-builtin.md, 20260714220213-resume-shell-face-slices-and-report.md]
origin_pr: 41
origin_pr_url: https://github.com/qmu/qfs/pull/41
origin_branch: work-20260714-111817
origin_commit: 7752cb3
created_at: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-15T16:35:34+09:00
last_seen: 2026-07-15T16:35:34+09:00
severity: low
status: active
resolved_by_pr:
resolved_by_commit:
---

# The `/type` catalog and the type resolver translate the stored key differently

## Description

`sys_drivers` stores a declared type's key in **path** form (`/type/chatwork/message`), which is what `of` normalises a bare name into; the catalog listing strips that prefix back to the **reference name** so it can be pasted into `of`. The two representations now coexist deliberately, and the first cut of the catalog got it wrong — printing the one spelling the grammar rejects (see [c20b6c4](https://github.com/qmu/qfs/commit/c20b6c4) in `packages/qfs/crates/qfs/src/type_catalog.rs`).

## How to Fix

Any future user-facing surface reading `sys_drivers` `kind='type'` rows owes the same translation; the §5.5 "paths are data, names are definitions" rule is a live encoding boundary here, not just a concept.
