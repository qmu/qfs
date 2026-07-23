---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-24T01:02:01+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: local-write-materialization-is-narrow
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
mission: 
---

# /local write materialization is narrow

## Description

Multi-column /local payloads without a named blob column still error (intentional narrow fallback); commit/effect content-blob threading not touched here

## How to Fix

Extend /local write materialization to support multi-column payloads without explicit blob columns

