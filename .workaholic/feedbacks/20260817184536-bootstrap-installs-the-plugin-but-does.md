---
type: Feedback
title: Bootstrap installs the plugin but does not guarantee it is bound
kind: concern
source: development
created_at: 2026-08-17T18:45:36+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: bootstrap-installs-the-plugin-but-does
owner: 
mission: []
tickets: [20260815130510-bootstrap-web-sessions-so-routines-can-run.md]
origin_pr: 45
origin_pr_url: https://github.com/qmu/qfs/pull/45
origin_branch: work-20260815-130504
origin_commit: cac7f05
last_seen: 2026-08-17T18:45:36+09:00
---

# Bootstrap installs the plugin but does not guarantee it is bound

## Description

The hook makes the plugin *installed* in a web session, not necessarily *bound* — a session binds whatever the registry named before SessionStart ran, and the issue #44 fallback (`plugins/workaholic/.../plugin-src.sh`, absent from this checkout) still exits 127 (see [f77898a](https://github.com/qmu/qfs/commit/f77898a) in `.claude/hooks/session-start.sh`).

## How to Fix

Treat the first post-merge tick as the proof; if it still pauses, pursue the harness-side rebind and the issue #44 fallback fix, never a local patch of the canonical hook.
