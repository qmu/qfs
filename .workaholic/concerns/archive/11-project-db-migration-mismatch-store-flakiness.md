---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-02T01:21:00+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: project-db-migration-mismatch-store-flakiness
severity: moderate
status: resolved
resolved_by_pr: 
resolved_by_commit: 
---

# project.db migration mismatch / store flakiness (203120)

## Description

A pre-existing `~/.config/qfs/project.db` migration mismatch surfaced intermittently during live verification; the migration guide and live-verification tickets (see [30e5ca7], [cd41ddb]) each worked around it with a fresh `XDG_CONFIG_HOME`. The forward-heal for Project v2's `1be5979f` in-place edit ([9b46d6c]) fixed one known checksum, but the underlying issue was never confirmed-ticketed and remains open. The CONNECT epic's migration 8 raises the stakes since project.db is now the single source of truth for path bindings.

## How to Fix

File/confirm a ticket for 203120, reproduce deterministically, and audit the migration runner's isolation. Every future in-place-edit-that-ships must add its own `SUPERSEDED_BODIES` entry; consider consolidating the runner given qfs is not a long-lived server.

## Resolution (2026-07-06)

All four asks are now covered:

- **Ticket filed + confirmed** — `20260706120200-project-db-migration-preship-guard` (the triage of
  this concern).
- **Deterministic reproduction** — already shipped: `crates/store/src/lib.rs`'s
  `superseded_body_is_healed_forward_not_rejected` rebuilds the exact v2 body and asserts the heal.
- **Runner isolation audited** — the `cfg(test)` `forbid_shared_home_fallback_in_tests()` guard
  (`crates/qfs/src/store.rs`) + the `IMMEDIATE`-transaction concurrency hardening shipped via
  `446d108`/`bff500d`; the runner is already a single consolidated `migrate()` for both scopes.
- **"Every future in-place edit must add a SUPERSEDED_BODIES entry" is now AUTOMATED** — the new
  `cargo xtask check-migrations` gate diffs each shipped `schema/*.sql` body against the last release
  tag and fails the build if a shipped body changed without a `SUPERSEDED_BODIES` heal-forward entry.
  Wired into the anti-drift gate family (CLAUDE.md). The forward-heal itself is `9b46d6c`.

Nothing left to do; archived.
