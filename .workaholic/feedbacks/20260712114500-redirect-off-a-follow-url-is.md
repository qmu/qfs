---
type: Feedback
title: Redirect off a follow URL is refused by the confined transport
kind: concern
source: development
created_at: 2026-07-12T11:45:00+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: redirect-off-a-follow-url-is
owner: 
mission: 
tickets: [20260712024651-resume-mission-close-out-gaps-and-live-rounds.md]
origin_pr: 35
origin_pr_url: https://github.com/qmu/qfs/pull/35
origin_branch: work-20260712-032443
origin_commit: c30fa0a
last_seen: 2026-07-16T16:14:56+09:00
closed: accepted
---

# Redirect off a follow URL is refused by the confined transport

## Description

FOLLOW-URL redirect refusal by the confined transport is unchanged; driver-http was not touched on this branch

## How to Fix

Implement redirect handling for FOLLOW URLs if security review approves
