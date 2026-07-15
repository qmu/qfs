---
created_at: 2026-07-04T17:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain, DB, Infrastructure]
effort: 4h
commit_hash: 4b96fb3
category: Added
depends_on: []
---

# Time-boxed cached vault unlock: enter the passphrase once, skip re-prompts for a bounded window

## Motivation

Every one-shot `qfs run` on `/local` re-opens the credential vault and, unless `QFS_PASSPHRASE`
is exported, re-prompts for the passphrase — annoying across repeated commands in a terminal.
The in-process cache (`PROMPTED_PASSPHRASE`, a `OnceLock` in
`packages/qfs/crates/qfs/src/connection.rs`) dies with the process, so a fresh command (or a new
tmux pane) re-asks; only the long-lived interactive shell prompts once per session.

The existing cross-invocation fix — the OS-keychain guardian slot (`qfs vault enroll keychain`) —
**does not help on this host**: it is a headless EC2 with no OS secret service, so `keychain_kek()`
returns `None`. It is also **permanent** (no time box), which is not what was asked. The request is
for a **limited-period authorized unlock** (ssh-agent / `sudo`-timestamp style) that works headless.

The KeyGuardian vault-slot seam was **built to accept this additively**: `slots.rs`'s design note
names "an agent or a managed KMS" as intended-later guardian kinds, and ADR 0008 lists
"passphrase + OS keychain now; agent/KMS later" (see
`.workaholic/tickets/archive/work-20260702-012808/20260702120020-keyguardian-vault-slots.md`).
This ticket is that later **time-boxed session** unlock — a new layer in front of the interactive
prompt, not a rewrite.

## Design (agreed with owner)

- **Mechanism: a time-boxed session file** (chosen over the permanent keychain slot, which is dead
  on this headless host, and over a heavier agent daemon). An ephemeral file under
  `$XDG_CONFIG_HOME/qfs/` (mode **0600**) holds the **DEK wrapped** under a short-lived,
  machine/session-bound key, plus a typed `expires_at`. **Cache the DEK, never the passphrase** —
  the argon2id derivation is the expensive step and the DEK is what actually opens sealed values.
- **Default TTL: 8 hours** (a work-day), overridable (e.g. `--ttl <dur>` and/or a config key). The
  session is minted at the moment `resolve_store_passphrase` successfully prompts, gated on
  `can_prompt_secret()` (a `QFS_PASSPHRASE`-env unlock must **not** silently mint a persistent
  cache).
- **Resolution ladder (compose, don't replace):** `QFS_PASSPHRASE` env → keychain guardian slot →
  **the new session-file unlock** → interactive prompt. Consult the session file in `open_store` /
  `open_store_for_commit` **before** the passphrase fallback, exactly mirroring the existing
  `keychain_kek()` branch.
- **Fail-closed:** an expired, unparseable, tampered, or wrong-owner session file is treated as
  **ABSENT** — fall through to the ladder (never a silent unlock, never a panic/hang). On a no-TTY
  headless host with no env var and no valid session, behavior stays **byte-identical to today**.
- **Explicit lock/revoke:** a `qfs vault lock` verb (and `qfs vault revoke` of the session
  guardian) purges the file immediately so the next command re-prompts. Presence + remaining TTL are
  inspectable (à la `qfs vault slots`) and never reveal key material.
- **Pure-core / binary-I/O split (dep-direction guard):** the expiry/decision logic (a typed
  `Deadline`, a `Valid | Expired | Corrupt | Absent` sum type) lives pure in `crates/secrets`
  (wasm-clean); the file/clock/`/dev/tty`/keychain I/O lives in the binary and is injected via the
  existing `open_with_resolver` / `kek_of` closure seam.

## Security constraints (Design/Implementation policy lens)

- Never write the passphrase or a **raw** KEK/DEK to plain disk; the on-disk material is itself
  envelope-protected and wrapped in the existing `Secret<T>` (Zeroize-on-drop, redacted `Debug`).
- Bind the session to the **OS user + host/session** so it cannot be replayed by another user or
  copied to another machine; zeroize + delete on expiry and on lock/revoke.
- The session file **must** be created mode `0600` with an owner-only re-check on open. **NOTE:** the
  broader gap that `project.db` itself is created world-readable under umask is split into its own
  ticket (`20260704170100-project-db-0600-permissions.md`); this ticket still creates and verifies
  **its own** cache file at 0600 regardless of that fix landing.
- No key material in argv or logs; audit events are masked; the existing nonce/ciphertext
  SELECT-redaction guard stays intact.

## Key files

- `packages/qfs/crates/qfs/src/connection.rs` — `resolve_store_passphrase` (mint site, ~:108-120),
  `PROMPTED_PASSPHRASE`, `open_store` (~:127), `open_store_for_commit` (~:209),
  `ensure_store_unlocked_for_scan` (~:181) — the consult sites.
- `packages/qfs/crates/qfs/src/vault.rs` — the `qfs vault` verb home (`enroll_keychain` as prior
  art); add `lock`/session-enroll + inspection.
- `packages/qfs/crates/qfs/src/secret_store.rs` — `SqliteSecrets` (DEK in memory, `open_with_resolver`,
  `enroll_slot`/`revoke_slot`).
- `packages/qfs/crates/secrets/src/{slots.rs,envelope.rs}` — pure guardian/slot + wrap/unwrap; add
  the pure expiry/`Deadline` logic here.
- `packages/qfs/crates/qfs/src/{tty.rs,store.rs}` — `/dev/tty` prompt + no-TTY gate; config-dir path
  for the session file.
- Reference TTL idiom: `invite --ttl` (`crates/cmd/src/lib.rs`), web sessions
  (`crates/qfs/src/session.rs`, `DEFAULT_SESSION_TTL_SECS`) — the repo's `expires_at`/seconds-TTL
  convention (note: `session.rs` is the SERVER web-session, unrelated to the vault — do not conflate).

## Quality Gate

1. **On-disk secrecy + 0600 (hermetic):** after an interactive unlock with the cache enabled,
   scanning the whole config dir and the session file's raw bytes for the known passphrase and the
   known DEK bytes yields **zero** hits, and `stat` on the session file reports mode **exactly 0600**
   owned by the current uid (any group/other bit or any plaintext-secret hit fails the gate).
2. **TTL + tamper fail-closed (hermetic + PTY):** with `expires_at` in the past (clock advanced past
   the TTL) and, separately, with the session file corrupted/truncated or owned by another uid, a
   fresh credential-needing `qfs run` re-prompts on a terminal / emits the honest locked-store hint
   on a headless host — never a silent unlock, never a panic. Pure expiry-boundary unit tests in
   `crates/secrets` (Valid at `t < deadline`, Expired at `t >= deadline`).
3. **Precedence + headless parity:** with `QFS_PASSPHRASE` exported the run uses the env var and
   writes **no** persistent session; on a no-TTY host with neither env var nor a valid session the
   behavior is byte-identical to today's fail-closed error; the existing `crates/cmd/tests/e2e_cli.rs`
   PTY / first-run / scan-time cases stay green.
4. **Lock/revoke + masked audit + full gate:** `qfs vault lock` makes the very next command
   re-prompt; cache hit/miss/expiry/evict emit structured audit events containing **no** key material
   (the `Secret<T>` Debug-redaction and nonce/ciphertext redaction assertions hold); and
   `cargo test --workspace`, `clippy -D warnings`, `fmt --check`, `gen-docs --check`,
   `gen-skills --check` all pass with the patch version bumped.
5. **Live confirmation (owner, once):** on this headless host, `qfs run` a credentialed read, enter
   the passphrase once; a second `qfs run` within the TTL does **not** re-prompt; after the TTL (or
   `qfs vault lock`) it re-prompts.
