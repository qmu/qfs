---
created_at: 2026-07-05T01:55:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 1h
commit_hash: d88851e
category: Changed
depends_on: []
---

# Auto-tighten a loose-but-owned credential store to 0600 on open, instead of refusing

## Problem (owner-reported, v0.0.20)

The v0.0.20 owner-only `0600` hardening (ticket 20260704170100) refuses to open a credential DB that
is group/other-accessible, naming a `chmod 600` remedy. Correct fail-closed posture, but **every
existing install created before v0.0.20 was mode 644 under the old umask, so the upgrade bricks the
CLI until the user manually `chmod 600`s each store** — the owner hit exactly this on their own host:

```
qfs: error: opening the project database: … credential DB ~/.config/qfs/project.db is
group/other-accessible (mode 644); refusing to use it — run `chmod 600 …` to restore owner-only access
```

(system.db has the same guard, so it re-appears on the next command.)

## Decision (owner-approved: option A)

**Self-heal by tightening, refuse only when we can't.** Tightening one's own credential DB from 644
to 600 is strictly safe (more restrictive) and is the exact remedy the error already prints — so do
it automatically on open rather than refusing. Keep refusing for the cases where tightening is not
ours to do or does not take:

- **Owned by us, loose bits** → `chmod` to `0600` and continue (the self-heal). This removes the
  upgrade friction.
- **Owned by another uid** → refuse. `std::fs::set_permissions` fails with `EPERM` for a file we do
  not own (only the owner or root can chmod), so relying on that failure needs no explicit uid
  syscall — a foreign-owned credential DB is a real problem, surfaced not silently tightened.
- **chmod did not take** (a mode-ignoring filesystem) → re-verify after the chmod and refuse if it is
  still group/other-accessible, with the manual `chmod 600` remedy.

Loosening is never auto-done; the guard only ever tightens. (qfs is experimental — this is a
straight behavior change, no compat shim.)

## Scope — both fail-closed sites (same pattern, apply consistently)

1. `packages/qfs/crates/store/src/fs_perms.rs` — `verify_owner_only` (the Project/System DB path,
   `store/src/lib.rs:141`; this is what the owner hit). The sidecars (`-wal`/`-shm`/journal) inherit
   from the tightened main file.
2. `packages/qfs/crates/secrets/src/local.rs` — its sibling `verify_owner_only` for the LocalStore
   credential blob (line ~298), so the same upgrade does not brick on the secrets blob next.

## Quality Gate

- A pre-existing group/other-accessible **owned** store DB / credential blob opens successfully and
  is left at exactly `0600` (hermetic tests over a tempdir, per-mode `0644/0640/0604/0666`).
- A file that cannot be tightened is still refused with a structured, secret-free error naming the
  manual `chmod 600` remedy (the `chmod`-didn't-take re-verify path; the foreign-owner EPERM path is
  covered by the same refusal message).
- The create-fresh-at-0600 path and the already-0600 idempotent-reopen path are unchanged.
- `cargo test --workspace`, `clippy --workspace --all-targets -- -D warnings`, `fmt --all --check`,
  `gen-docs --check`, `gen-skills --check` all green. Bump the patch to 0.0.21 before the PR.

## Key files

- `packages/qfs/crates/store/src/fs_perms.rs` (`verify_owner_only` + its tests; module doc)
- `packages/qfs/crates/store/src/lib.rs` (the FileSource reject test ~line 946 → now a tighten test)
- `packages/qfs/crates/secrets/src/local.rs` (`verify_owner_only` ~line 298 + its test ~line 445)
