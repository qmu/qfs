---
type: Feedback
title: /cf live (203090) unimplemented; /cf and /rest are placeholder mounts
kind: concern
source: development
created_at: 2026-07-02T01:21:00+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: cf-live-203090-unimplemented-cf-and
owner: 
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: []
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
last_seen: 2026-07-24T01:08:52+09:00
closed: resolved
resolved_by_pr: 17
resolved_by_commit: ff2085d
---

# /cf live (203090) unimplemented; /cf and /rest are placeholder mounts

## Description

/cf and /rest remain placeholder mounts pending a richer connection declaration and owner CF token; untouched by this branch

## How to Fix

Implement /cf with a live Cloudflare account and a richer connection declaration grammar
