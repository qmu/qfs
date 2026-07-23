---
type: Concern
concern_id: the-branch-safety-scanner-false-positives
mission: 
tickets: [20260713195008-effect-selector-channel-folder-rename.md, 20260714120000-effect-selector-uniform-migration.md, 20260714154144-general-of-type-assertion.md, 20260714182710-shell-face-slice1-ls-cat-describe-typed.md, 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md, 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md, 20260714182740-shell-face-type-mount-and-describe-builtin.md, 20260714220213-resume-shell-face-slices-and-report.md]
origin_pr: 41
origin_pr_url: https://github.com/qmu/qfs/pull/41
origin_branch: work-20260714-111817
origin_commit: 7752cb3
created_at: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-15T16:35:34+09:00
last_seen: 2026-07-23T23:59:51+09:00
severity: moderate
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# The branch-safety scanner false-positives on Rust `Token::Variant`, hard-blocking `/ship`

## Description

The precision bug is in the workaholic plugin's secret-patterns.sh (a different repo) and cannot be fixed from qfs; unaddressed and still hard-blocks /ship on Rust Token::Variant tokens — this branch adds lexer Token:: usages in document.rs that may trip it

## How to Fix

Fix the false-positive pattern in the workaholic plugin's secret-patterns.sh (ticket already filed in qmu/workaholic)

