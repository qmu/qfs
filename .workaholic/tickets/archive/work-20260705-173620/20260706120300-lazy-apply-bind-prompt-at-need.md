---
created_at: 2026-07-06T12:03:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
category: Changed
depends_on: []
---

# Lazy, prompt-at-proven-need bind for the commit-side apply registry (converges concerns 15 + 21)

## What's wanted

Reads already bind lazily (register a `LazyCloudReadDriver`, unlock only when a query provably
scans that mount, prompt at need). The APPLY/commit side does not: `commit.rs::live_registry`
opens the credential store eagerly at registry build via the quiet, never-prompt primitive for
EVERY connected cloud + declared driver. Result: a terminal `qfs run --commit` against a cloud
mount with a locked store (no `QFS_PASSPHRASE`) leaves the driver silently unregistered and the
commit fails with a generic "no driver" error — never a locked-store hint, never a prompt. Two
concerns (PR#15 cloud apply, PR#21 declared-driver connect) are the same defect in one function;
fix them together.

## Current state (verified against HEAD 61f696c)

- Pattern to copy (read side): `crates/qfs/src/shell.rs:223-244` + `LazyCloudReadDriver` (288-361);
  `crates/qfs/src/connection.rs:185-210` (`ensure_store_unlocked_for_scan` — the prompt-at-need
  primitive).
- Eager apply-side opens: `commit.rs:26-27` (`apply_plan` builds `live_registry` up front),
  `commit.rs:313-320` (`register_cloud_mounts` over every cloud mount), `commit.rs:600-640`
  (`networked_credential` -> `open_store_for_commit`, "never prompts"), `commit.rs:499-506`
  (objstore SigV4 secret resolved eagerly), `commit.rs:329-351` + `declared_driver.rs:559-564`
  (`declared_secrets` once per declared driver — the PR#21 site), plus `google.rs:96,207` and
  `clients.rs:35,52`.
- No `Lazy*ApplyDriver` exists yet.

## Implementation steps

1. Introduce a `LazyCloudApplyDriver` (and declared/REST equivalent) implementing
   `ApplyDriver::apply_batch/apply_one`, deferring the real bind + unlock to first use, cached in a
   `OnceLock` — symmetric to the read side (`mount_adapter.rs:351-376` confirms the wrap shape).
2. Unlock via a prompt-capable primitive (reuse `ensure_store_unlocked_for_scan` or an
   apply-flavored twin) instead of the quiet-only `open_store_for_commit`.
3. Convert every eager site above (cloud, objstore SigV4, declared, google, clients) to the lazy
   wrapper.
4. Tests: a locked-store `--commit` against an unused cloud mount succeeds; against a used one it
   prompts (tty) or emits a structured locked-store error (non-tty), never a generic "no driver".

## Key files

- `crates/qfs/src/commit.rs`, `crates/qfs/src/shell.rs`, `crates/qfs/src/connection.rs`,
  `crates/qfs/src/declared_driver.rs`, `crates/qfs/src/google.rs`, `crates/qfs/src/clients.rs`,
  `crates/qfs/src/mount_adapter.rs`.

## Considerations

- Source concerns: `.workaholic/concerns/15-commit-side-apply-registry-still-binds.md` and
  `.workaholic/concerns/21-declared-driver-live-read-apply-eagerly.md` (both resolve together).
