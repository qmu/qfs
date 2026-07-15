---
created_at: 2026-07-04T17:01:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 1h
commit_hash: cad336f
category: Added
depends_on: []
---

# Vault store files are created world-readable under umask — enforce 0600 + owner-only

## Motivation

Surfaced by the policy discovery for the time-boxed vault-unlock ticket
(`20260704170000-timeboxed-session-vault-unlock.md`). The credential store `project.db`
(`$XDG_CONFIG_HOME/qfs/project.db`) is created via `FileSource::connect()` in
`packages/qfs/crates/store/src/lib.rs` **without an explicit mode**, so it inherits the process
umask and is typically **world/group-readable** on disk. Although the secrets inside are
envelope-encrypted (argon2id KEK + ChaCha20-Poly1305 DEK), a credential-bearing file being
world-readable is a defense-in-depth gap (RFD §10 secret hygiene / least-privilege): it leaks
metadata, salts, and ciphertext to any local user and sets a bad precedent for the new
credential-adjacent cache file the sibling ticket adds.

Split from the unlock ticket at the owner's request so the permissions hardening lands on its own.

## Fix

- Create the store DB (and any sibling credential file) mode **0600** at open/init time, and
  **re-check owner-only** on every open (fail closed with a structured error if the file is
  group/other-readable or owned by another uid) — mirror the legacy `LocalStore::verify_owner_only()`
  pattern the repo already has for local file sources.
- Provide this as a small shared helper (e.g. in `crates/store` or a fs-permissions util) so the
  session-file cache in `20260704170000` reuses the same 0600-create + owner-check discipline
  instead of hand-rolling permissions.
- Unix-only concern; on non-Unix, degrade gracefully (no-op or best-effort) without failing the open.

## Key files

- `packages/qfs/crates/store/src/lib.rs` — `FileSource::connect()` (the create-without-mode site)
  and the Project/System DB open path.
- `packages/qfs/crates/qfs/src/store.rs` — resolves the `$XDG_CONFIG_HOME/qfs/` DB paths.
- Prior art: the legacy `LocalStore::verify_owner_only()` owner-only re-check pattern (grep the tree).

## Quality Gate

- **0600 on create (hermetic):** after opening/initialising a fresh store in a temp `XDG_CONFIG_HOME`,
  `stat` reports the DB mode is **exactly 0600** owned by the current uid — asserted regardless of the
  test process umask (set a permissive umask in the test to prove the mode is explicit, not inherited).
- **Owner-only re-check fails closed:** a store file `chmod`ed to `0644` (or `0640`) is **rejected**
  on the next open with a structured, value-free error — never silently used.
- **No regression:** existing store/secret-store tests stay green; `cargo test --workspace`,
  `clippy -D warnings`, `fmt --check`, `gen-docs --check`, `gen-skills --check` pass; patch bumped.
- Non-Unix build still compiles (the permissions step is `#[cfg(unix)]`-gated or a graceful no-op).
