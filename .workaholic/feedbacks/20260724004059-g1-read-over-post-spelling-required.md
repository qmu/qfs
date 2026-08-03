---
type: Feedback
title: G1 read-over-POST spelling required refinement during implementation
kind: concern
source: development
created_at: 2026-07-24T00:40:59+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: g1-read-over-post-spelling-required
owner: a@qmu.jp
mission: [the-declared-driver-dsl-covers-the-compiled-drivers-concisely]
tickets: [20260722091100-coverage-inventory-of-compiled-driver-surfaces.md, 20260722091200-rule-the-semantic-gaps-in-blueprint-13.md, 20260722091300-ship-read-over-post-hermetically.md, 20260722091400-conciseness-bar-stated-and-measured.md, 20260722091500-conversion-playbook-and-honest-tiering.md]
origin_pr: 21
origin_pr_url: https://github.com/qmu/qfs/pull/21
origin_branch: work-20260722-084646
origin_commit: f52592b
last_seen: 2026-07-28T13:09:57+09:00
---

# G1 read-over-POST spelling required refinement during implementation

## Description

The second half of this concern — that downstream rulings carry the same implementation-detail precision — was **actively contradicted** during this branch: G2, G4 and G5 all shipped here and none carried a shipped-note until the release pass added them (see [3ac4508](https://github.com/qmu/qfs/commit/3ac4508)). The drift recurred exactly as the concern warned.

## How to Fix

Make the shipped-note part of the definition of done for any blueprint ruling a branch implements, rather than a release-pass cleanup.
