---
status: active
severity: low
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 
concern_id: the-config-comment-stripper-truncates-paths
origin_pr: 30
origin_pr_url: https://github.com/qmu/qfs/pull/30
origin_branch: work-20260707-180554
origin_commit: e7e44ee
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# The config `--` comment stripper truncates paths containing `--`

## Description

The `.qfs` config statement splitter strips from the first `--` on a line even inside a path token, so a statement like `DO REMOVE /local/a--b/x POLICY p` silently loses its tail (path truncated, POLICY clause dropped). Observed as two `qfs` job-test failures whenever `$TMPDIR` contains `--` (sandbox scratchpads do); green under a clean TMPDIR and in CI. For a REMOVE this mis-addresses the target… (`the-config-comment-stripper-truncates-paths.md`, origin `e7e44ee`)

## How to Fix

Make the comment stripper quote/token-aware (a `--` inside a quoted string or path token is not a comment), or require whitespace before `--` to open a comment; add a regression test with a `--`-bearing path.

