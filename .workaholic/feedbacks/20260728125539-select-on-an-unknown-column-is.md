---
type: Feedback
title: `select` on an unknown column is still silently dropped
kind: concern
source: development
created_at: 2026-07-28T12:55:39+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: select-on-an-unknown-column-is
owner: a@qmu.jp
mission: [a-where-predicate-is-honored-or-refused-never-dropped]
tickets: [20260717180100-where-on-an-unknown-column-returns-zero-rows-at-exit-0.md, 20260717180200-expand-silently-no-ops-on-json-and-unknown-columns.md, 20260717180300-codec-source-error-names-the-pre-decode-columns.md, 20260723020055-gdrive-where-pushdown-silent-drop.md]
origin_pr: 26
origin_pr_url: https://github.com/qmu/qfs/pull/26
origin_branch: work-20260724-011029
origin_commit: ee5af0f
last_seen: 2026-07-28T12:55:39+09:00
---

# `select` on an unknown column is still silently dropped

## Description

`where` and `expand` now refuse an unknown column, and `/sql` already refused one in `select`, but the generic engine still drops an unknown `select` column silently — an only-unknown projection returns the row count with an empty schema (see [c6f531c](https://github.com/qmu/qfs/commit/c6f531c)). This is the last member of the defect family this mission set out to close, and the behaviour is already inconsistent between drivers today.

## How to Fix

Rule it — refuse, consistent with `where`/`expand` and with `/sql`, or keep the documented silent drop as deliberate leniency over heterogeneous and late-bound relations. The judgement, not the code, is the cost. Ticketed at `20260725113000-select-on-an-unknown-column-is-silently-dropped.md`.
