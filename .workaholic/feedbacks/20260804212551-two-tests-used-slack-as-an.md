---
type: Feedback
title: Two tests used Slack as an example of a general property
kind: concern
source: development
created_at: 2026-08-04T21:25:51+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: two-tests-used-slack-as-an
owner: a@qmu.jp
mission: [a-declared-write-resolves-a-name-the-way-a-query-does]
tickets: [20260726190000-declared-reverse-lookup-for-write-path-name-resolution.md, 20260725103000-declared-expand-must-splice-by-field-name.md, 20260726090000-map-body-expressions-can-reference-path-params.md, 20260724014100-slack-call-maps-effect-equivalent.md, 20260724014200-retire-the-compiled-slack-driver.md]
origin_pr: 31
origin_pr_url: https://github.com/qmu/qfs/pull/31
origin_branch: work-20260803-213737
origin_commit: 6d505d1
last_seen: 2026-08-04T21:25:51+09:00
---

# Two tests used Slack as an example of a general property

## Description

`describe.rs`'s compiled-versus-declared shadowing test and `golden_corpus.rs`'s

## How to Fix

When a component is on a retirement path, prefer a stable example elsewhere; an
