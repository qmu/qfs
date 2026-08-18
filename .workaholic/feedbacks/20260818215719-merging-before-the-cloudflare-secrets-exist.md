---
type: Feedback
title: Merging before the Cloudflare secrets exist turns `main` red
kind: concern
source: development
subject: observer_ai:a@qmu.jp
created_at: 2026-08-18T21:57:19+09:00
author: a@qmu.jp
supersedes:
severity: urgent
concern_id: merging-before-the-cloudflare-secrets-exist
owner: a@qmu.jp
mission: [the-documentation-site-publishes-itself-staging-on-merge-production-on-release]
tickets: [20260817142443-a-merge-to-main-publishes-the-docs-site-to-staging-qfs-qmu-co-jp.md, 20260817142443-a-release-publishes-the-docs-site-to-qfs-qmu-co-jp.md, 20260817142443-the-docs-deployment-is-recorded-where-the-github-release-already-is.md, 20260817142443-the-docs-site-has-a-worker-deploy-target-it-can-be-published-to.md, 20260817164716-a-tag-can-publish-reference-docs-that-drifted-from-the-binary.md]
origin_pr: 74
origin_pr_url: https://github.com/qmu/qfs/pull/74
origin_branch: work-20260817-163919
origin_commit: a44d4db
last_seen: 2026-08-18T21:57:19+09:00
---

# Merging before the Cloudflare secrets exist turns `main` red

## Description

A failed deploy fails `docs-build` by design, so with `CLOUDFLARE_API_TOKEN`

## How to Fix

Set both repository secrets and publish once by hand before merging.
