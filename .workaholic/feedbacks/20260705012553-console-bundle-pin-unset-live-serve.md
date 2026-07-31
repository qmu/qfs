---
type: Feedback
title: Console bundle pin unset; live serve + release stamp pending the plgg bundle
kind: concern
source: development
created_at: 2026-07-05T01:25:53+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: console-bundle-pin-unset-live-serve
owner: 
mission: 
tickets: []
origin_pr: 18
origin_pr_url: https://github.com/qmu/qfs/pull/18
origin_branch: work-20260704-181053
origin_commit: 72c8950
last_seen: 2026-07-28T13:09:57+09:00
---

# Console bundle pin unset; live serve + release stamp pending the plgg bundle

## Description

`PINNED_BUNDLE` is still the empty coordinate, pinned by its own test (see [72c8950](https://github.com/qmu/qfs/commit/72c8950)).

## How to Fix

Pin the console bundle once the plgg bundle carries a release stamp.
