---
created_at: 2026-07-03T07:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [UX, Domain]
effort: 1h
commit_hash: 96c936a
category: Changed
depends_on: []
---

# Cloud reads must unlock the vault at scan time (prompt on a terminal)

First-user finding (owner, v0.0.16 local loop): after a successful consent and
`qfs connect /drive`, `qfs run "/drive |> limit 5"` on a terminal WITHOUT `QFS_PASSPHRASE`
failed with "this Drive mount has no usable Google account" — misleading: the account exists,
sealed behind the locked vault. The read-side mount bind opens the store only through the
quiet, never-prompting paths (`open_store_for_commit`), so a locked store silently failed the
bind at registry build.

## Fix

Keep the registry build prompt-free (it runs for every `qfs run`, including credential-free
previews). Defer the failed bind to SCAN time via a `LazyCloudReadDriver`
(`crates/qfs/src/shell.rs`): when the executing query provably reads the cloud mount, unlock
the store — quiet paths first, else a one-time `/dev/tty` prompt cached process-wide
(`connection::ensure_store_unlocked_for_scan`) — then rebuild and delegate to the live facet.
Cache only a SUCCESSFUL bind so a shell session can retry. When the store still cannot unlock
(headless, no env var), fail with an honest locked-store hint instead of the connect hint.

## Quality Gate

- Hermetic: with the store quietly unlockable but no app/account, the lazy scan reports the
  per-kind connect hint on the scan's own path (never the locked-store hint).
- PTY e2e drives the real binary: setup with the env passphrase, then a cloud read with it
  unset prompts "QFS passphrase" at scan time and, after the unlock, surfaces the honest
  connect hint; the locked-store hint never fires.
- Workspace tests / clippy / fmt / gen-docs / gen-skills green; the owner's live
  `/drive |> limit 5` with a prompt is the acceptance proof.
