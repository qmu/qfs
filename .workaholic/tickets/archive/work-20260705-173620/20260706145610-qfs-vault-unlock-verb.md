---
created_at: 2026-07-06T14:56:10+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 2h
commit_hash: 69a2d70
category: Added
depends_on:
---

# `qfs auth` — warm the time-boxed session on demand

## Overview

Add a **top-level `qfs auth`** command that unlocks the credential store through the existing
guardian ladder (an echo-off passphrase prompt when needed) and **force-mints** the time-boxed
session-unlock cache (default 8h TTL, ticket 20260704170000), then prints the resulting session
status/TTL line. Its inverse, `qfs auth --lock`, drops the session.

The motivating workflow: the operator does **not** drive `qfs` interactively — a delegated AI agent
issues `qfs` one-shot invocations. The operator wants **one short, memorable command** to run up
front each morning, entering the passphrase once, so the agent's *separate-process* one-shots ride
the on-disk `session.unlock` file for the TTL window without ever seeing or re-prompting the
passphrase. Today the 8h session is minted **only as a side effect** of an interactive prompt inside
a store-opening command — there is no dedicated command to warm it deliberately.

**CLI shape (settled with the operator — see Discussion, Revision 1).** The command is the top-level
**`qfs auth`**, *not* a `vault` subverb: the operator types it daily and needs it short and memorable
(their original ask — "do we have a `qfs auth` command?"). The session lock/unlock is conceptually
the *ephemeral auth session*, distinct from `qfs vault`, which reverts to pure **persistent key-slot**
management (slots / enroll / revoke / rekey). So:

- `qfs auth` → warm the 8h session.
- `qfs auth --lock` → drop it.
- The `qfs vault lock` / `qfs vault unlock` verbs are **removed** (experimental, no back-compat —
  hard breaks are correct). The internal `VaultAction::Lock` / `VaultAction::Unlock` actions the
  binary launcher handles are **kept** — now reached via `qfs auth`, reusing the injected
  `VaultLauncher` (no new launcher plumbing).

**Mint semantics (settled at ticket time):** `qfs auth` mints/refreshes the session whenever it
successfully unlocks the store **by any guardian** — the echo-off passphrase prompt, but also
`QFS_PASSPHRASE`, an enrolled OS keychain slot, or an already-live session. An explicit `auth`
invocation is an intentional "warm it now" signal, distinct from the parent ticket's rule that a
`QFS_PASSPHRASE`-env unlock must never *silently* mint during ordinary implicit store-opens. This
requires force-minting from the unlocked store rather than relying on the `JUST_PROMPTED`-gated
`maybe_mint_session`, because `open_store()`'s keychain / live-session guardians short-circuit
**before** the passphrase branch and never set that flag.

## Policies

The standard engineering policies (synced from qmu.co.jp into the `workaholic` policy skills) that
govern this ticket. The implementing session **MUST** read each linked hard copy before writing code
and keep every change defensible against that policy's Goal (目標), Responsibility (責務), and
Practices (実践).

- `workaholic:implementation` / `policies/directory-structure.md` — the change lands only in the
  established slots: verb parse in `crates/cmd/src/lib.rs`, vault I/O in
  `crates/qfs/src/{vault.rs,session_unlock.rs,connection.rs}`, guide prose in `docs/guide/`.
- `workaholic:implementation` / `policies/coding-standards.md` — Rust style/idiom matching the
  surrounding `vault.rs` / `session_unlock.rs` (doc-comments in the existing voice, no new deps).
- `workaholic:implementation` / `policies/domain-layer-separation.md` — `qfs-cmd` stays thin:
  it only adds the backend-free `VaultAction::Unlock` enum arm and its parse; **all** secret/store/
  session I/O lives in the binary's injected `run_vault` launcher (the `dep_direction` guard forbids
  `qfs-cmd` from touching `qfs-secrets`).
- `workaholic:implementation` / `policies/type-driven-design.md` — the entered passphrase flows as
  `qfs_secrets::Secret` (redacting `Debug`, no `Serialize`, zeroize-on-drop); only the AEAD-wrapped
  DEK reaches disk, never the passphrase.
- `workaholic:implementation` / `policies/functional-programming.md` — reuse the existing pure
  expiry/record model (`qfs_secrets::session::classify`) and the I/O-owning `mint` /
  `resolved_ttl_secs` / `status_line` helpers rather than re-deriving crypto or file I/O.
- `workaholic:implementation` / `policies/objective-documentation.md` — the clap doc-comment and any
  guide entry state actual, verifiable behavior (prompts echo-off, mints an 8h session, prints
  status).
- `workaholic:implementation` / `policies/test.md` — hermetic regression tests beside the domain
  (verb dispatch/mapping + `force_mint` mints without `JUST_PROMPTED`), not proof-by-many-green.
- `workaholic:design` / `policies/defense-in-depth.md` — the verb sits on the secret-vs-sink trust
  boundary and mutates the on-disk ephemeral cache; every gate defaults to refusal (no tty + no env
  ⇒ clear error, never a hang), and the minted file inherits the fail-closed guarantees (0600,
  uid/deadline-bound KEK, purge-on-tamper, reboot-invalidation).
- `workaholic:design` / `policies/self-explanatory-ui.md` — `unlock` reads as the mirror of `lock`;
  the printed status shows presence + remaining TTL, and an idempotent re-run reports state rather
  than erroring.
- `workaholic:design` / `policies/access-control.md` — proportionate: minting *requires* a real
  unlock (a wrong passphrase fails the unlock and mints nothing).
- `workaholic:safety` / `policies/standard.md` — the passphrase is read echo-off from the
  controlling terminal, never from argv/stdout/logs; `VaultAction::Unlock` carries no secret on
  argv. Matches the documented posture in `docs/security/threat-model.md` and
  `docs/guide/passphrase.md`.
- `workaholic:operation` / `policies/ci-cd.md` — the shipped-PR operational rules apply: bump the
  patch in `crates/qfs/crates/qfs/Cargo.toml`; the anti-drift checks (`gen-docs --check`,
  `gen-skills --check`) must stay green.

## Key Files

- `packages/qfs/crates/cmd/src/lib.rs` — the clap surface. Add a **top-level** `Command::Auth {
  lock: bool }` variant (in the `Command` enum, alongside `Init`/`Vault`) with a `--lock` flag, and
  a `Command::Auth` dispatch arm (near the `Command::Vault` arm, ~967) that forwards to the existing
  `vault` launcher: `vault(&if lock { VaultAction::Lock } else { VaultAction::Unlock })`. **Remove**
  `VaultVerb::Lock` and `VaultVerb::Unlock` and their `vault_action` mapping arms (the `vault`
  namespace reverts to slots/enroll/revoke/rekey). **Keep** `VaultAction::Lock` and
  `VaultAction::Unlock` — they are now reached via `qfs auth`, not a `vault` verb.
- `packages/qfs/crates/qfs/src/vault.rs` — the injected `VaultLauncher`. Keep the
  `VaultAction::Lock => lock_session()` / `VaultAction::Unlock => unlock_session()` arms and the
  `unlock_session()` handler (modeled on `lock_session()`); only their CLI trigger changes (from a
  `vault` verb to `qfs auth`). `run_vault` already `println!`s the returned `String`.
- `packages/qfs/crates/qfs/src/session_unlock.rs` — the session-cache I/O. Add a
  `force_mint_session(store)` sibling of `maybe_mint_session` (~236–256) that mints via the existing
  `mint` / `resolved_ttl_secs` / `now_epoch` / `current_uid` / `session_unlock_path` **without**
  consulting `JUST_PROMPTED`, and emits the same `SESSION_MINT` audit. Re-read `status_line`
  (261–276) after minting for the printed TTL.
- `packages/qfs/crates/qfs/src/connection.rs` — `open_store()` (131–170) is the guardian ladder the
  handler drives; unchanged. Note the load-bearing subtlety it documents: guardians 1 (keychain) and
  2 (live session) short-circuit **before** the passphrase branch and never mint — the reason
  `force_mint_session` is needed instead of the `maybe_mint_session` at line 168.
- `packages/qfs/crates/secrets/src/session.rs` — the pure `SessionRecord` / `classify`; unchanged
  (no schema change), but it defines the record `status_line` renders.
- `packages/qfs/crates/qfs/src/main.rs` — wires `&vault::run_vault` as the launcher (~line 80);
  unchanged, confirms the injection seam.
- `docs/guide/passphrase.md`, `docs/guide/cli.md` — hand-written reference. Document `qfs auth` /
  `qfs auth --lock` as the session commands (hand-written prose, **not** generated — see
  Considerations). Drop any `vault lock`/`vault unlock` mention.
- `packages/qfs/crates/qfs/Cargo.toml` — bump the patch version for the shipped PR.

## Related History

The time-boxed session mechanism and the `vault` verb namespace already exist; this ticket adds the
one missing verb (`unlock`) on top of shipped seams — no new mechanism, table, or migration.

Past tickets that built the foundation this reuses:

- [20260704170000-timeboxed-session-vault-unlock.md](.workaholic/tickets/archive/work-20260704-181053/20260704170000-timeboxed-session-vault-unlock.md) — **direct parent.** Built the whole session-unlock mechanism (0600 `session.unlock`, DEK wrapping, 8h `DEFAULT_TTL_SECS`, fail-closed expiry/tamper, `maybe_mint_session`, `status_line`, `purge_session`) **and** the symmetric `qfs vault lock` verb — but mints only as a passive side effect; no dedicated unlock verb.
- [20260702120020-keyguardian-vault-slots.md](.workaholic/tickets/archive/work-20260702-012808/20260702120020-keyguardian-vault-slots.md) — established the `qfs vault` verb namespace, `VaultAction`/`VaultVerb`, and the injected `run_vault` launcher pattern the new arm slots into.
- [20260703021500-passphrase-prompt-dev-tty.md](.workaholic/tickets/archive/work-20260703-022500/20260703021500-passphrase-prompt-dev-tty.md) — owns the echo-off `/dev/tty` passphrase prompt (`tty::prompt_secret`, `can_prompt_secret`) and the can-prompt gate this verb reuses.
- [20260702120000-epic-adr0008-multi-host-account-model.md](.workaholic/tickets/archive/work-20260702-012808/20260702120000-epic-adr0008-multi-host-account-model.md) — the parent EPIC defining ADR 0008 §5's guardian model that the session layer extends additively.

## Implementation Steps

1. **`crates/cmd/src/lib.rs` — the top-level `auth` command.** Add `Command::Auth { #[arg(long =
   "lock")] lock: bool }` to the `Command` enum with a doc-comment (warm the 8h session; `--lock`
   drops it). Add its dispatch arm near `Command::Vault`: `vault(&if lock { VaultAction::Lock } else
   { VaultAction::Unlock })`. **Remove** `VaultVerb::Lock` and `VaultVerb::Unlock` and their
   `vault_action` mapping arms. **Keep** `VaultAction::Lock` / `VaultAction::Unlock`. Selectors only
   — no passphrase/KEK on argv.
2. **`crates/qfs/src/session_unlock.rs` — force-mint helper.** Add `pub fn force_mint_session(store:
   &SqliteSecrets) -> Option<i64>` next to `maybe_mint_session`: mint via `mint(&path, store,
   resolved_ttl_secs(), now_epoch(), current_uid())` **without** the `JUST_PROMPTED` gate, emit
   `emit_connection_audit("SESSION_MINT", "vault")` on success, return the deadline. Reuse
   `session_unlock_path()` (returns `None` on a host with no session dir — handle gracefully).
   *(Done — unchanged by the `auth` rename; the handler logic is CLI-shape-independent.)*
3. **`crates/qfs/src/vault.rs` — handlers (unchanged).** The `VaultAction::Unlock => unlock_session()`
   / `VaultAction::Lock => lock_session()` arms and the `unlock_session()` handler stay as-is; only
   their CLI trigger moves to `qfs auth`. `unlock_session()` calls `connection::open_store()?` (the
   guardian ladder — prompts echo-off only if no keychain/live-session/env path unlocks first), then
   `session_unlock::force_mint_session(&store)`, then returns a message embedding
   `session_unlock::status_line()` (graceful fallback when the mint / `status_line()` returns `None`).
4. **Fail-closed check.** Confirm `qfs auth` surfaces `open_store()`'s existing structured
   secret-free error verbatim when there is no tty **and** no `QFS_PASSPHRASE` (headless, unset) —
   never a hang, exit 1.
5. **Tests (hermetic).** (a) In `crates/cmd/src/lib.rs`, add dispatch assertions that `qfs auth`
   routes to the vault launcher as `VaultAction::Unlock` and `qfs auth --lock` as `VaultAction::Lock`
   (via the stub launcher); drop the removed `qfs vault lock`/`unlock` assertions. (b) In
   `session_unlock.rs`, the `force_mint_session` unit test (mints even with `JUST_PROMPTED` false;
   `status_line()` / `session_unlock_material()` then report it — contrast: `maybe_mint_session`
   would not). *(Done.)*
6. **Docs (hand-written).** Document `qfs auth` / `qfs auth --lock` as the session commands in
   `docs/guide/passphrase.md` and `docs/guide/cli.md`; remove the `vault lock`/`unlock` mentions.
   Verifiable, behavior-accurate prose.
7. **Version + anti-drift.** Patch bumped in `packages/qfs/crates/qfs/Cargo.toml` (0.0.23 → 0.0.24).
   `cargo run -p xtask -- gen-docs --check` and `gen-skills --check` stay green (CLI help is not
   rendered into the generated reference, and the qfs skills do not teach these commands, so no
   regen/plugin re-version is triggered; if either check flags drift, regenerate and, per CLAUDE.md,
   bump the four plugin `version` fields).

## Quality Gate

Captured at ticket time (the operator delegated the specific gate — the recommended "suite + live
proof" gate is adopted). `/drive` surfaces this in its approval prompt and forwards it into the
commit `Verify:` key. Every line is objective and verifiable.

**Acceptance criteria** — the checkable conditions that must hold:

- `qfs auth` mints a fresh time-boxed session and prints a status line reporting a remaining TTL of
  ~8h (or the `QFS_SESSION_TTL` override), from **any** successful unlock — including a
  `QFS_PASSPHRASE`-env unlock with no tty.
- After `qfs auth`, a **subsequent separate-process** `qfs` invocation that needs the credential
  store resolves **without** re-prompting (rides guardian 2, the session cache), within the TTL.
- `qfs auth --lock` after an `auth` drops the session; the next command re-prompts (round-trip
  symmetry).
- On a headless host with no tty **and** no `QFS_PASSPHRASE`, `qfs auth` fails closed with the
  existing structured, secret-free error and exit code 1 — no hang.
- `force_mint_session` mints when `JUST_PROMPTED` is false (unit-proven); the on-disk record leaks
  no plaintext key material (existing session invariants still hold).
- `qfs-cmd` gains no dependency on `qfs-secrets` (the `dep_direction` test stays green); the `vault`
  namespace no longer exposes `lock`/`unlock`.

**Verification method** — the commands/tests/probes that prove them:

- `cargo test --workspace` green, including the new `crates/cmd` `qfs auth` dispatch assertions and
  the new `session_unlock` `force_mint` unit test.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean
  (my files; the tree also carries a concurrent session's `commit.rs`/`shell.rs` WIP that I do not
  touch — see Discussion).
- `cargo run -p xtask -- gen-docs --check` and `cargo run -p xtask -- gen-skills --check` green.
- **Live in-session demo** over a hermetic `XDG_CONFIG_HOME` (temp dir): `QFS_PASSPHRASE=… qfs init`
  (or a first credential write to create the store), then `QFS_PASSPHRASE=… qfs auth` prints the ~8h
  status; then a subsequent `qfs` one-shot (no `QFS_PASSPHRASE`) that touches the store resolves via
  the session; then `qfs auth` with `QFS_PASSPHRASE` **unset and no tty** fails closed; then
  `qfs auth --lock` drops it and the same one-shot then fails closed.

**Gate** — what must pass before approval at `/drive`:

- The full suite (test + clippy + fmt) and both anti-drift checks are green, **and** the live
  in-session demo above is exercised and its behavior observed (per the `verify` skill), not just
  asserted by tests.

## Considerations

- **Force-mint vs. the parent's env invariant** (`crates/qfs/src/session_unlock.rs`,
  `crates/qfs/src/connection.rs` lines 117–120). The parent ticket kept a `QFS_PASSPHRASE`-env
  unlock from *silently* minting during implicit store-opens. `unlock` intentionally mints from env
  too, but only under an **explicit** invocation — keep `maybe_mint_session`'s `JUST_PROMPTED` gate
  untouched for the implicit paths; add a **separate** `force_mint_session` rather than loosening the
  existing gate, so no implicit path starts minting from env as a side effect.
- **No new `--ttl` flag.** `unlock` honors the existing `QFS_SESSION_TTL` env override (clamped
  1m..7d) via `resolved_ttl_secs`, keeping one TTL-override path and a bare, memorable command. A
  `--ttl` flag can be a later follow-up if wanted.
- **Keychain host is a near no-op.** On a host with an enrolled OS keychain slot the store already
  unlocks non-interactively every command, so the session cache is largely moot there; `unlock`
  still force-mints (harmless refresh) for uniform behavior.
- **Docs are hand-written, not generated** (`docs/guide/passphrase.md`, `docs/guide/cli.md`).
  `gen-docs` renders only `language.md`/`drivers.md`/`server.md` from the binary's registries — the
  vault CLI is documented by hand, so step 6 is a manual prose edit, not a regen. Verify
  `gen-docs --check` stays green regardless.
- **Plugin re-version is not triggered by default.** The qfs Agent Skills teach `vault slots`/
  `rekey`, not the session commands; adding `qfs auth` without editing a `docs/cookbook/*.md` article
  leaves `gen-skills --check` green and needs no plugin `version` bump. Only the binary patch bump is
  required. If a cookbook article ends up teaching `qfs auth`, then re-version the four plugin fields
  per CLAUDE.md.

## Discussion

### Revision 1 - 2026-07-06T15:57:01+09:00

**User feedback** (verbatim): "So you mean I need to input 'qfs value unlock' every morning? You
know that I cannot accept that, shorter like qfs auth".

**Ticket updates**:
- Overview rewritten: the command is a **top-level `qfs auth`** (with `qfs auth --lock` as the
  inverse), not a `qfs vault unlock` subverb. This is the operator's original ask ("do we have a
  `qfs auth` command?") and the daily-typed command must be short.
- Implementation Step 1 rewritten: add `Command::Auth { lock }` + its dispatch arm routing through
  the existing `vault` launcher (`VaultAction::Unlock` / `VaultAction::Lock`); **remove**
  `VaultVerb::Lock` / `VaultVerb::Unlock` and their `vault_action` arms; **keep** the internal
  `VaultAction` variants and the `vault.rs` handlers.
- Step 3 marked unchanged (handler logic is CLI-shape-independent); Steps 5–6 and the Quality Gate
  reworded from `qfs vault unlock`/`lock` to `qfs auth`/`qfs auth --lock`.

**Direction change**: The ephemeral session is now its own top-level concept (`qfs auth`), and the
`qfs vault` namespace reverts to pure persistent key-slot management (slots/enroll/revoke/rekey).
Removing the just-added `qfs vault unlock` **and** the previously-shipped `qfs vault lock` is a hard
break, which is correct for this experimental pre-release (no back-compat). The session
mechanism (`force_mint_session`, `unlock_session`/`lock_session`, the 8h cache) is unchanged — only
the CLI front door moves.

## Final Report

Development completed as planned (with the `qfs auth` reshape from Revision 1). Shipped:

- **`qfs auth`** (top-level) warms the time-boxed session by force-minting the 8h cache from any
  successful store unlock; **`qfs auth --lock`** drops it. Both route through the existing injected
  `VaultLauncher` (`VaultAction::Unlock` / `VaultAction::Lock`) — no new launcher threaded through
  `run()`. The `vault lock` / `vault unlock` verbs were removed; `qfs vault` is key-slots only.
- **`force_mint_session`** added to `session_unlock.rs` — a `JUST_PROMPTED`-free sibling of
  `maybe_mint_session`; the implicit-path gate is untouched.
- Docs (`cli.md`, `passphrase.md`) present `qfs auth` / `qfs auth --lock` as the session commands.
- Patch bump `0.0.23 → 0.0.24`.

Gate cleared: `cargo test --workspace` green (0 failures) incl. the new `qfs auth` dispatch tests and
the `force_mint_session` unit test; `cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo fmt --all --check` clean; `gen-docs`/`gen-skills --check` in sync; live in-session demo over a
throwaway `XDG_CONFIG_HOME` confirmed the full lifecycle (warm 8h / 2h, separate-process one-shot
rides the session, `--lock` drops it, headless fails closed) with correct exit codes.

### Discovered Insights

- **Insight**: A new top-level CLI command that only needs store/vault I/O can reuse the existing
  injected `VaultLauncher` by mapping to a `VaultAction`, instead of threading a new launcher param
  through `qfs_cmd::run()` and every call site/test.
  **Context**: `run()` takes ~14 injected launchers; adding one is broad churn. `qfs auth` dispatches
  `vault(&if lock { VaultAction::Lock } else { VaultAction::Unlock })`, so the CLI surface and the
  backend dispatch stay decoupled — a verb's *namespace* is independent of which launcher serves it.
- **Insight**: The cross-process `session.unlock` file is minted only via `JUST_PROMPTED` (an
  interactive prompt); a non-interactive unlock (env/keychain/live) never mints. A command that must
  *deliberately* warm the session needs an explicit force-mint from the already-unlocked store, not
  reliance on that gate.
  **Context**: `open_store()`'s keychain and live-session guardians short-circuit before the
  passphrase branch that sets the flag — hence `force_mint_session`.
- **Insight**: `docs/guide/*.md` (the hand-written CLI reference) is NOT covered by the `gen-docs`
  anti-drift check (which only renders `language.md`/`drivers.md`/`server.md`), so a CLI reshape
  needs manual guide edits and will not trip `gen-docs --check`.
  **Context**: relevant whenever a CLI verb is added/renamed — the generated reference won't flag it.
