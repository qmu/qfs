---
type: Concern
concern_id: definition-catalog-cp-clone-and-mv
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

# Definition-catalog `cp`=clone and `mv`=rename are refused, not implemented

## Description

An owner-approved deviation from slice 3's settled design. A definition row **carries its own name**, so `cp /transform/a /transform/b` would re-insert `a` rather than clone it to `b`; and no in-place rename exists to lower onto (`/type` exposes no write verb at all, `/transform` has no `UPDATE`). Both refuse, naming re-declaration instead (see [fc99572](https://github.com/qmu/qfs/commit/fc99572) in `packages/qfs/crates/exec/src/shell/desugar.rs`).

## How to Fix

If def-rename is wanted, add a name-rewriting projection the shell can build; until then the refusal is the honest floor, since a silent copy+delete would leave every reference to the old name dangling.
