---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-02T01:21:00+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: cloud-reads-panicked-under-runtime-within
severity: moderate
status: resolved
resolved_by_pr: 33
resolved_by_commit: a59f914
---

# Cloud reads panicked under runtime-within-runtime blocking

## Description

Every cloud read facet's client drives the shared reqwest transport via its own `block_on`; called from inside the async read executor (itself a tokio worker) this panics with "Cannot start a runtime from within a runtime" (see [613c1f5] and [cf08355]). Only objstore was guarded, so gmail/gdrive/ga/github/slack live reads crashed the process; the hermetic mock-client path never exercised it. Fixed on this branch, but the class is easy to reintroduce.

## How to Fix

Run any blocking transport call on a dedicated OS thread (`std::thread::scope`) with no tokio context, reducing a panic to a structured secret-free error. Apply the same treatment to every future blocking-transport integration.

## Resolution (2026-07-12, PR #33)

The predicted reintroduction happened: driver-http's sync `ReqwestClient` drove its owned runtime
with `block_on`, panicking inside the exec commit boundary's runtime and silently blocking every
live `|> transform` commit. Fixed at `a59f914` with the concern's own prescription —
`Handle::try_current` detects the ambient runtime and drives the owned runtime on a scoped worker
thread, a joined panic degrading to a structured secret-free transport error — locked by a loopback
ambient-runtime wire regression test in `driver-http/tests/wire.rs`.
