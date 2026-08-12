---
created_at: 2026-08-13T03:32:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
---

# The default `qfs auth` session lasts two weeks, and a two-week override is reachable at all

## Overview

Developer instruction, 2026-08-13 (feedback
`20260813033130-the-default-qfs-auth-session-ttl-must-be-two-weeks-not-8-hours.md`): the default
time-boxed session `qfs auth` mints must be **two weeks**, not the 8 hours it has had since the
mechanism shipped.

`qfs auth` unlocks the credential store once and caches that unlock in a `0600`
`session.unlock` file so later one-shots — a new pane, or a delegated agent's separate processes —
skip the passphrase prompt until it expires. The window is `DEFAULT_TTL_SECS`, today `8 * 60 * 60`.

Two things are wrong, and the second is the one that makes this more than a constant edit:

1. **The default is 8h.** It was chosen when the mechanism was built and never revisited.
2. **Two weeks is unreachable even with the documented override.** `resolved_ttl_secs` clamps
   `QFS_SESSION_TTL` to `MAX_TTL_SECS = 7 * 24 * 60 * 60`, so `QFS_SESSION_TTL=14d qfs auth` yields
   **7d** — and the clamp is silent, so the operator sees a number they did not ask for with nothing
   saying it was reduced. Whoever tried to self-serve the two-week window would have been told
   nothing.

## Scope

**In scope.**

1. `DEFAULT_TTL_SECS` becomes two weeks.
2. `MAX_TTL_SECS` rises so the new default is not itself clamped, and so an operator can ask for
   more than the default if they want it. Pick the ceiling deliberately and say why in the constant's
   doc comment — a ceiling exists to bound the blast radius of a stolen session file, so "no ceiling"
   is not the answer.
3. **A clamped TTL says so.** When `QFS_SESSION_TTL` is reduced (or raised) by the clamp, the
   resulting status line — or a line beside it — states that the requested value was adjusted and to
   what. A silent adjustment is the defect that hid (2) for weeks.
4. Every place that teaches "8h" is corrected: the `Auth` command doc comment in
   `crates/cmd/src/lib.rs` (both the enum variant's doc and the module-level mention), the guide
   pages, and any test asserting the old default.

**Out of scope, and stated so it is a decision rather than an omission.** Whether a session should
survive a **reboot**. The session KEK folds in `/proc/sys/kernel/random/boot_id`, so *any* TTL is in
practice "until the next reboot". A two-week default makes that binding much more visible: on a
long-lived server it rarely bites, on a laptop it means the two weeks are nominal. Changing it is a
security decision (the boot binding is what makes a copied session file useless after a reboot), so
it is left to the developer rather than quietly bundled into a TTL change.

## Key Files

- `packages/qfs/crates/qfs/src/session_unlock.rs` — `DEFAULT_TTL_SECS`, `MIN_TTL_SECS`,
  `MAX_TTL_SECS`, `resolved_ttl_secs`, `status_line`, `derive_session_kek` (the boot-id binding).
- `packages/qfs/crates/cmd/src/lib.rs` — the `Auth` variant's doc comment ("default 8h, override with
  `QFS_SESSION_TTL`"), which is what `qfs auth --help` prints.
- `packages/qfs/crates/qfs/src/vault.rs` — `unlock_session` / `lock_session`, which print the status.
- `docs/guide/passphrase.md`, `docs/guide/cli.md` — the operator-facing text naming the window.

## Implementation Steps

1. Change the two constants; state the ceiling's reason in its doc comment.
2. Make the clamp observable: `resolved_ttl_secs` reports whether it adjusted the requested value,
   and the `qfs auth` output says so when it did.
3. Update the `Auth` doc comment and the guide pages; regenerate the reference docs.
4. Tests: the default is two weeks; `QFS_SESSION_TTL=14d` yields exactly two weeks (the regression
   this ticket exists for); a value above the ceiling is clamped **and reported**; a garbled value
   still falls back to the default rather than disabling the cache; the existing expiry/tamper
   fail-closed tests still pass unchanged.

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — a silently clamped TTL is the engine
  answering a different question than the one asked. Either honor the value or say what was done.
- `workaholic:implementation` / objective-documentation — `qfs auth --help` currently teaches "8h" as
  a fact; every surface that names the window has to move with the constant.
- `workaholic:safety` — the window is a security parameter: lengthening it widens the value of a
  stolen `session.unlock`. The mitigations that remain (0600 + uid ownership, the AEAD-authenticated
  deadline, machine-id and boot-id binding) are what the longer window rests on, and the ceiling
  exists for the same reason.

## Quality Gate

1. **Acceptance:** a bare `qfs auth` on a host with no `QFS_SESSION_TTL` prints a remaining TTL of
   two weeks.
2. **Acceptance:** `QFS_SESSION_TTL=14d qfs auth` yields two weeks, not seven days.
3. **Acceptance:** a request above the ceiling is clamped **and the output says it was clamped and to
   what**.
4. **Acceptance:** no surface still teaches 8h (`qfs auth --help`, the guide pages, generated docs).
5. **Verification:** hermetic unit tests over `resolved_ttl_secs` and the status rendering; no live
   vault needed.
6. **Gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Considerations

- The deadline is baked in at mint time and authenticated by the KEK derivation, so this changes
  nothing for sessions already on disk: an existing 8h session keeps its 8h deadline until it expires
  or `qfs auth` is re-run. Worth one sentence in the guide so nobody reports it as the change not
  working.
- The reboot question above is the one thing that could make a two-week default feel broken in
  practice. If the developer wants sessions to survive reboots, that is a follow-up ticket against
  `derive_session_kek`, with its own security note.
