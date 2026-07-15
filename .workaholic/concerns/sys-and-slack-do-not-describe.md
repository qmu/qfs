---
type: Concern
concern_id: sys-and-slack-do-not-describe
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

# `/sys` and `/slack` do not describe their roots, so `cd` there fails before the gate

## Description

`describe /sys` returns `unsupported_verb` and `/slack`'s root is not a node at all, so `cd /sys` fails at describe rather than at the enumerable-children gate. Slack additionally has no "channel tree" node — every addressable node is a leaf, and `files` is already navigable as a `BlobNamespace` (see [fb664b5](https://github.com/qmu/qfs/commit/fb664b5)).

## How to Fix

Decide whether `/sys` and `/slack` roots should become describable catalog nodes; that is new driver surface (a root node plus its schema), not a flag, which is why slice 2's quality gate named only the four reachable interiors.
