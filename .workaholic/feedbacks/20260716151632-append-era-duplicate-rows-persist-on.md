---
type: Feedback
title: Append-era duplicate rows persist on disk but resolve correctly
kind: concern
source: development
created_at: 2026-07-16T15:16:32+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: append-era-duplicate-rows-persist-on
owner: 
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260715190000-resume-development-in-the-new-public-repo.md, 20260716005029-unify-the-qfs-statement-splitter.md, 20260716120200-reinstall-replaces-a-declaration.md]
origin_pr: 1
origin_pr_url: https://github.com/qmu/qfs/pull/1
origin_branch: work-20260715-205333
origin_commit: ddb419e
last_seen: 2026-07-28T13:09:57+09:00
---

# Append-era duplicate rows persist on disk but resolve correctly

## Description

No `UNINSTALL`/`DROP DRIVER` grammar exists at HEAD, so superseded append-era rows still have no compaction path (see [ddb419e](https://github.com/qmu/qfs/commit/ddb419e)).

## How to Fix

Implement a bundle-aware uninstall surface that removes superseded rows.
