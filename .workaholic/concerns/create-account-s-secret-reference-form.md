---
type: Concern
concern_id: create-account-s-secret-reference-form
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260715190000-resume-development-in-the-new-public-repo.md, 20260716005029-unify-the-qfs-statement-splitter.md, 20260716120200-reinstall-replaces-a-declaration.md]
origin_pr: 1
origin_pr_url: https://github.com/qmu/qfs/pull/1
origin_branch: work-20260715-205333
origin_commit: ddb419e
created_at: 2026-07-16T15:16:32+09:00
first_seen: 2026-07-16T15:16:32+09:00
last_seen: 2026-07-16T15:16:32+09:00
severity: low
status: active
resolved_by_pr:
resolved_by_commit:
---

# (carried from an unrecorded PR) CREATE ACCOUNT's SECRET reference form is unimplemented

## Description

The CREATE ACCOUNT SECRET '&lt;ref&gt;' clause is still unimplemented (needs bind-time account-credential resolution); the parser grammar was not touched on this branch

## How to Fix

Implement SECRET clause resolution at bind time during account creation
