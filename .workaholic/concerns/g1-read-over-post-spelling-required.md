---
type: Concern
concern_id: g1-read-over-post-spelling-required
mission: [the-declared-driver-dsl-covers-the-compiled-drivers-concisely]
owner: a@qmu.jp
tickets: [20260722091100-coverage-inventory-of-compiled-driver-surfaces.md, 20260722091200-rule-the-semantic-gaps-in-blueprint-13.md, 20260722091300-ship-read-over-post-hermetically.md, 20260722091400-conciseness-bar-stated-and-measured.md, 20260722091500-conversion-playbook-and-honest-tiering.md]
origin_pr: 21
origin_pr_url: https://github.com/qmu/qfs/pull/21
origin_branch: work-20260722-084646
origin_commit: f52592b
created_at: 2026-07-24T00:40:59+09:00
first_seen: 2026-07-24T00:40:59+09:00
last_seen: 2026-07-24T00:40:59+09:00
severity: moderate
status: active
resolved_by_pr:
resolved_by_commit:
---

# G1 read-over-POST spelling required refinement during implementation

## Description

The initial ruling stated read-over-POST as a bare source clause; during implementation it was refined to a `|> POST { body }` pipe-op form (see [e5cd1a3](https://github.com/qmu/qfs/commit/e5cd1a3)). The ruling specification was incomplete at recording time and needed clarification during proof.

## How to Fix

Document in blueprint §13.1 the reason for the pipe-op form (tier-2 idiom for quirks minimizes AST churn) and ensure downstream rulings include similar implementation-detail precision to avoid surprises during proof.
