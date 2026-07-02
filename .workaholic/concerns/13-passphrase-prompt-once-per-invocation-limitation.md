---
type: Concern
origin_pr: 13
origin_pr_url: https://github.com/qmu/qfs/pull/13
origin_branch: work-20260703-022500
origin_commit: 92a5e30
created_at: 2026-07-03T05:41:25+09:00
severity: low
status: active
resolved_by_pr:
resolved_by_commit:
---

# Passphrase prompt once-per-invocation limitation on headless hosts

## Description

The per-one-shot prompt asks once per invocation; on headless hosts without a secret service the export remains the practical path for long sessions (see [9b04649](https://github.com/qmu/qfs/commit/9b04649) in `packages/qfs/crates/qfs/src/tty.rs`).

## How to Fix

For long-running headless sessions (cron, CI, persistent daemons), recommend the `read -rs QFS_PASSPHRASE; export` pattern; getting-started now clarifies when this is needed versus terminal prompting.
