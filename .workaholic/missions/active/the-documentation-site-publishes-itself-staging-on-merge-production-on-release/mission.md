---
type: Mission
title: The documentation site publishes itself: staging on merge, production on release
slug: the-documentation-site-publishes-itself-staging-on-merge-production-on-release
status: active
merge_policy:
created_at: 2026-08-17T14:24:13+00:00
author: noreply@anthropic.com
assignees: [a@qmu.jp]
assignee:
predicted_hours:
actual_hours: 1.5
feedback: [20260817142308-auto-deploy-docs-to-staging-qfs-qmu-co-jp-on-merge-to-main-and-to-qfs-qmu-co-jp-on-release.md]
tickets: []
stories: []
gate_type:
gate_assert:
gate_target:
claim: work-20260818-224556
---

# The documentation site publishes itself: staging on merge, production on release

## Goal

The docs site is built by CI on every push and read by nobody but the person who runs
`docker compose up docs`. The ask is that the two states the repository already
distinguishes — what is on `main`, and what a `v*` tag released — each reach a hostname of
their own, published by a worker rather than by hand.

## Experience

A merge to `main` lands, and minutes later staging-qfs.qmu.co.jp serves that commit's
documentation. A `v0.0.x` tag is pushed, the release builds, and qfs.qmu.co.jp serves the
released documentation. Nobody runs a deploy command; both URLs say which commit they carry,
and the repository documents the procedure the way it documents the GitHub Release.

## Acceptance

- [x] A merge to `main` publishes the built site to staging-qfs.qmu.co.jp automatically. (#20260817142443-a-merge-to-main-publishes-the-docs-site-to-staging-qfs-qmu-co-jp.md)
- [x] A `v*` release publishes the same built site to qfs.qmu.co.jp automatically. (#20260817142443-a-release-publishes-the-docs-site-to-qfs-qmu-co-jp.md)
- [x] The procedure and its credentials are recorded in `.workaholic/deployments/`. (#20260817142443-the-docs-deployment-is-recorded-where-the-github-release-already-is.md)

## Changelog

- 2026-08-17 — Proposed from feedback 20260817142308 (issue #69).
- 2026-08-17 — ticket archived — 20260817142443-the-docs-site-has-a-worker-deploy-target-it-can-be-published-to.md
- 2026-08-17 — ticket archived — 20260817142443-a-merge-to-main-publishes-the-docs-site-to-staging-qfs-qmu-co-jp.md
- 2026-08-17 — ticket archived — 20260817142443-a-release-publishes-the-docs-site-to-qfs-qmu-co-jp.md
- 2026-08-17 — ticket archived — 20260817142443-the-docs-deployment-is-recorded-where-the-github-release-already-is.md
- 2026-08-17 — run recorded (+0.3h) — work-20260817-163919
- 2026-08-17 — story added — work-20260817-163919.md
- 2026-08-18 — ticket archived — 20260817164716-a-tag-can-publish-reference-docs-that-drifted-from-the-binary.md
- 2026-08-18 — run recorded (+0.4h) — session_01XW34NxhnCKHuHMseJjwq2E
- 2026-08-18 — concern deferred (stuck) — 20260818215719-merging-before-the-cloudflare-secrets-exist.md
- 2026-08-18 — concern deferred (stuck) — 20260818215719-one-deployment-record-carries-two-environments.md
- 2026-08-18 — concern deferred (stuck) — 20260818215719-wrangler-is-installed-on-every-branch.md
- 2026-08-18 — story added — work-20260818-143402.md
- 2026-08-18 — run recorded (+0.4h) — session_01TgM27Y8Bz21tWg2j3DoxS7
- 2026-08-18 — story added — work-20260818-194054.md
- 2026-08-18 — run recorded (+0.2h) — session_017KjfyG1SFQamWhS7h6QbLZ
- 2026-08-18 — run recorded (+0.2h) — session_01FKn9idn6673AsxqXfeywjQ
- 2026-08-19 — concern deferred (stuck) — 20260819145408-the-docs-ci-token-still-holds.md
- 2026-08-19 — concern deferred (stuck) — 20260819145408-staging-s-non-indexability-rests-on.md
