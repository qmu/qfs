---
type: Feedback
title: Staging's non-indexability rests on one mechanism, not two
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-08-19T14:54:08+09:00
author: a@qmu.jp
supersedes:
severity: low
concern_id: staging-s-non-indexability-rests-on
owner: a@qmu.jp
mission: [the-documentation-site-publishes-itself-staging-on-merge-production-on-release]
tickets: []
origin_pr: 99
origin_pr_url: https://github.com/qmu/qfs/pull/99
origin_branch: work-20260818-224556
origin_commit: 56b25c3
last_seen: 2026-08-19T14:54:08+09:00
---

# Staging's non-indexability rests on one mechanism, not two

## Description

The design intended `robots.txt` and `X-Robots-Tag` as two independent guards.

## How to Fix

Either turn off the zone's managed robots.txt, which restores the origin
