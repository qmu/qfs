---
created_at: 2026-07-06T12:02:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 2h
commit_hash: 0b15532
category: Added
depends_on: []
---

# project.db migration: pre-ship guard against silent in-place body edits (203120)

## What's wanted

Confirm-ticket for concern 203120. The known checksum-mismatch case is already fixed
(heal-forward `9b46d6c`, test-isolation guard `446d108`), and the migration runner is already a
single consolidated function. The one real remaining gap: nothing catches a FUTURE developer
editing an already-shipped migration body before merge — the runtime `ChecksumMismatch`/heal path
only fires against accumulated real state, and every CI test uses a fresh DB that never trips it.

## Current state (verified against HEAD 61f696c)

- Runner: `crates/store/src/migrate.rs:90-196` (one `migrate()` used by both SystemDb + ProjectDb).
- Heal registry keyed on old-body checksum: `migrate.rs:46-73`; heal-vs-fail branch 140-151. Policy
  prose (migrate.rs:15-25) requires a new `SUPERSEDED_BODIES` entry per shipped in-place edit, but
  it is NOT automated.
- Git-history audit: only `project_secrets.sql` was ever edited in place (already healed); no other
  shipped migration body has changed.
- Deterministic repro already exists: `crates/store/src/lib.rs:632-716`
  (`superseded_body_is_healed_forward_not_rejected`).

## Implementation steps

1. Add an `xtask` check that, for each already-shipped migration version, diffs its embedded
   `Migration::sql` body against the last release tag's embedded body and fails the build if a
   shipped version's checksum changed without a matching new `SUPERSEDED_BODIES` entry.
2. Wire it into the anti-drift gate family (alongside `gen-docs`/`gen-skills --check`).
3. Update concern `11-project-db-migration-mismatch-store-flakiness` to resolved-narrowed (link
   `9b46d6c` + `446d108`), or close it.

## Key files

- `crates/store/src/migrate.rs`, `crates/store/src/lib.rs`, `crates/qfs/src/store.rs`,
  `crates/xtask/*`, `.workaholic/concerns/11-project-db-migration-mismatch-store-flakiness.md`.

## Considerations

- "Consolidate the runner" is already done (one function serves both scopes). Scope here is the
  pre-ship guard only.
- Source concern: `.workaholic/concerns/11-project-db-migration-mismatch-store-flakiness.md`.
