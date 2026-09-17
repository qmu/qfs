---
type: Feedback
title: Every push runs the CI matrix twice, and the pull_request copy can hold a pull request out of mergeability
kind: instruction
source: development
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-19T05:58:04+00:00
author: a@qmu.jp
supersedes: 
---

# Every push runs the CI matrix twice, and the pull_request copy can hold a pull request out of mergeability

# Every push runs the CI matrix twice, and the pull_request copy can hold a pull request out of mergeability

`.github/workflows/ci.yml` triggers on both `push: branches: ["**"]` and `pull_request`, and no workflow in `.github/workflows/` declares a `concurrency:` group. Every commit on a pull request branch therefore runs the full eight-job matrix twice, on the same SHA, and the two copies race for runners.

Measured on 2026-08-19 during housekeeping tick 20260819-055201, on #101 at head 21f3c747: the push-event run 32217535724 completed green in 3m28s, its `cross-compile (aarch64-unknown-linux-gnu)` job taking 2m53s. The pull_request-event run 32217538517, started three seconds earlier, still had that same job `in_progress` sixty minutes later, with the other seven green. GitHub derives `mergeable_state` from the pull_request run, so #101 read `unstable` — the state the housekeeping stuck-prs step reports as `checks` — for an hour with nothing actually failing and nothing left to run except a duplicate of a job that had already passed on the same bytes. Nothing in the loop can clear it: a stalled job ends on the six-hour runner timeout or on a human re-running it.

The ask is to make one run per SHA decide mergeability: add a `concurrency:` group keyed on the workflow and the ref with `cancel-in-progress: true`, or drop the duplicate trigger (the conventional `push: branches: [main]` plus `pull_request`) so a branch commit runs the matrix once. Either halves the runner minutes every pull request spends and removes a whole class of pull requests that sit unmergeable behind a check that already passed.
