---
type: Feedback
title: Auto-deploy docs to staging-qfs.qmu.co.jp on merge to main, and to qfs.qmu.co.jp on release
kind: instruction
source: slack
subject: person:claude[bot]
created_at: 2026-08-17T13:20:04+00:00
author: a@qmu.jp
supersedes: 
---

# Auto-deploy docs to staging-qfs.qmu.co.jp on merge to main, and to qfs.qmu.co.jp on release

# Auto-deploy docs to staging-qfs.qmu.co.jp on merge to main, and to qfs.qmu.co.jp on release

A direction was filed on this repository (qmu/qfs) for the documentation site: it should
publish itself, on two triggers and to two hostnames. On **every merge to `main`**, the
documentation is to be deployed automatically — via a worker — to
`staging-qfs.qmu.co.jp`. On **every merge that produces a release** (the binary release
cycle this project already runs, tag `vX.Y.Z` → `release.yml` → GitHub Release), the same
mechanism is to deploy the documentation to `qfs.qmu.co.jp`.

Today the docs (repo-root `docs/`, VitePress) are served only locally
(`docker compose up docs` at `localhost:5173`); nothing publishes them anywhere, and the
repository has no deploy workflow for them. The ask names the two triggers, the two
hostnames, and the deployment vehicle ("a worker"); it does not name the account, the
project names, or how the two environments are separated.

Source: https://github.com/qmu/qfs/issues/69
