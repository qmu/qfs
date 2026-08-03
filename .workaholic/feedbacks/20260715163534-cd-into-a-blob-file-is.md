---
type: Feedback
title: `cd` into a blob file is still admitted
kind: concern
source: development
created_at: 2026-07-15T16:35:34+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: cd-into-a-blob-file-is
owner: 
mission: 
tickets: [20260713195008-effect-selector-channel-folder-rename.md, 20260714120000-effect-selector-uniform-migration.md, 20260714154144-general-of-type-assertion.md, 20260714182710-shell-face-slice1-ls-cat-describe-typed.md, 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md, 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md, 20260714182740-shell-face-type-mount-and-describe-builtin.md, 20260714220213-resume-shell-face-slices-and-report.md]
origin_pr: 41
origin_pr_url: https://github.com/qmu/qfs/pull/41
origin_branch: work-20260714-111817
origin_commit: 7752cb3
last_seen: 2026-07-28T13:09:57+09:00
closed: superseded
---

# `cd` into a blob file is still admitted

## Description

driver-local's describe is still path-agnostic and returns `BlobNamespace` unconditionally (see [7752cb3](https://github.com/qmu/qfs/commit/7752cb3)).

## How to Fix

Refuse `namespace=BlobNamespace` at `cd` time in describe.
