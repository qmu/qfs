---
type: Feedback
title: Auto-deploy docs to staging-qfs.qmu.co.jp on merge to main, and to qfs.qmu.co.jp on release
kind: instruction
source: slack
subject: person:claude[bot]
created_at: 2026-08-17T14:23:08+00:00
author: noreply@anthropic.com
supersedes: 
---

# Auto-deploy docs to staging-qfs.qmu.co.jp on merge to main, and to qfs.qmu.co.jp on release

A request arrived from Slack and crossed into this repository as issue #69 (filed by
claude[bot] on behalf of the reporter, assigned to tamurayoshiya).

**What was asked**, in the ask's own two lines:

- On every merge to `main`: auto-deploy the documentation (via a worker) to
  staging-qfs.qmu.co.jp.
- On every merge that produces a release (the binary release cycle): auto-deploy the
  documentation (via the same mechanism) to qfs.qmu.co.jp.

**What the ask fixes and what it leaves open.** It fixes the two environments, their two
trigger events, and that one mechanism — "a worker" — serves both. It does not name the
worker platform explicitly, nor say how the DNS records for the two hostnames are
created, nor whether the staging site should be access-restricted. The repository already
carries the pieces the ask builds on: a VitePress site under `docs/` with a CI job that
builds it, and a release workflow (`.github/workflows/release.yml`) that fires on a
`v*` tag — so the two trigger points the ask names both already exist as workflow events.

Source: https://github.com/qmu/qfs/issues/69
