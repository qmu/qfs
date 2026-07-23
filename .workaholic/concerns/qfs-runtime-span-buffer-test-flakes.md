---
type: Concern
mission: 
tickets: [20260708013532-cf-artifacts-repositories-as-a-resource.md, 20260708192730-transform-definition-ddl-storage.md, 20260708192731-transform-plan-spine.md, 20260708192732-transform-execution-routing.md, 20260708192733-transform-docs-versioning-live-run.md, 20260709024731-fix-sqlite-busy-flake-pragma-order.md, 20260709054542-resume-transform-epic-review-fixes-and-t3-t4.md, 20260709104254-blueprint-type-system-chapter.md, 20260709104255-two-layer-model-stage-admission-test.md, 20260709104256-reference-convention-transform-surface.md, 20260709104257-arithmetic-operators.md, 20260709104258-stdlib-naming-resolution-like-eq.md, 20260709104259-pipeline-valued-lambdas-decision.md, 20260709104300-transform-one-seam-lock.md, 20260709140000-column-type-refined-name-resolution.md]
origin_pr: 32
origin_pr_url: https://github.com/qmu/qfs/pull/32
origin_branch: work-20260709-023822
origin_commit: 22c61e4
created_at: 2026-07-11T04:39:49+09:00
last_seen: 2026-07-24T00:40:59+09:00
first_seen: 2026-07-11T04:39:49+09:00
concern_id: qfs-runtime-span-buffer-test-flakes
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# qfs-runtime span-buffer test flakes under parallel workspace tests

## Description

The qfs-runtime shared-span-buffer test-isolation flake is unaddressed; the runtime crate was not modified on this branch

## How to Fix

Add test isolation for the shared span buffer to prevent flakes in parallel test runs

