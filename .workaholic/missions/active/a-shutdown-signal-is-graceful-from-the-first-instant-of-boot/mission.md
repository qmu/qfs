---
type: Mission
title: A shutdown signal is graceful from the first instant of boot
slug: a-shutdown-signal-is-graceful-from-the-first-instant-of-boot
status: active
merge_policy:
created_at: 2026-08-18T13:21:25+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
assignee:
predicted_hours:
actual_hours:
feedback: [20260818131946-a-sigint-shutdown-e2e-test-flakes-and-reddens-main.md]
tickets: []
stories: []
gate_type:
gate_target:
gate_assert:
---

# A shutdown signal is graceful from the first instant of boot

## Goal

A merge to `main` went red at random: `serve_boots_mixed_fixture_and_drains_audit_on_sigint`
failed on PR #74's merge commit with `ExitStatus(unix_wait_status(2))`, then passed on the
next push over the same tree.

The mechanism says this is not only a test defect: the shutdown listener is installed inside
`Runtime::run()` (`crates/server/src/runtime.rs:382`), after boot and after the
`server running` line a readiness wait would key on. A signal landing during boot meets the
default disposition and kills the process un-drained — what raw wait status 2 means, so the
`t36` "a `systemctl stop` is a clean drain" contract does not hold while booting. Five sites
in `e2e_serve.rs` / `e2e_binding_ddl.rs` guess readiness with a fixed `sleep`.

## Experience

`qfs serve` takes the graceful path — stop accepting, drain the audit ledger, exit 0 — for a
SIGINT or SIGTERM delivered at any instant after the process starts, boot included; and the
serve e2e tests wait for observable readiness rather than a guessed interval, so a loaded CI
runner cannot redden a merge commit.

## Acceptance

- [ ] A shutdown signal delivered during boot takes the graceful path (exit 0, ledger
      drained), not the default disposition — covered by a test that signals mid-boot. (#20260818132305-a-shutdown-signal-arriving-during-boot-takes-the-graceful-path.md)
- [ ] No serve e2e test decides readiness with a fixed sleep; each waits on an observable
      readiness signal with a bounded timeout that fails loudly. (#20260818132310-the-serve-e2e-tests-wait-for-observable-readiness-never-a-fixed-sleep.md)

## Changelog

<!-- Append-only, dated timeline. One line per event; never rewrite past lines. -->
