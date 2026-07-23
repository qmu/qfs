---
created_at: 2026-07-19T10:12:01+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: a-request-resolves-to-a-principal-the-query-path-can-read
---

# The identity read-back tells the truth

Satisfies mission acceptance: **"The identity read-back tells the truth."**
Two silent-wrong-answer defects on the surface this mission extends, recorded in the
mission's *Measured starting state* and re-verified against the tree at `cab0197`
(binary `qfs 0.0.81`):

1. **`qfs identity whoami --json` accepts `--json` and silently emits prose.** The
   global `--json` flag (`cmd/src/lib.rs`, `cli.json`) is not threaded into
   `IdentityAction::Whoami`, so `run_identity` (`crates/qfs/src/identity.rs:26`) always
   `println!`s the human string `"{email} (user {id})"`. The flag is declared and accepted
   at exit 0 but has no effect. This directly undercuts mission Experience 5 (the principal
   answer must be machine-readable) — the nearest existing identity read-back silently is not.
2. **`qfs identity --help` asserts a retired sign-up and a pending t46.** The clap doc on
   the `Identity` subcommand (`cmd/src/lib.rs:702-706`) renders verbatim into user-visible
   help and still says *"sign up (email + password)"*, *"there is local sign-up"*, and
   *"sessions land in t46"* — signup is retired (ADR 0008; `qfs init` replaced it) and t46
   has shipped. Commit `91cde7d` swept two other instances but did not reach this one.

## Implementation

- **Thread `--json` to whoami.** Add a `json: bool` field to
  `IdentityAction::Whoami` (`cmd/src/lib.rs:262-266`). Set it from `cli.json` where the
  `Identity` command is dispatched (`cmd/src/lib.rs:1143` → pass `cli.json` into
  `identity_action`). Update `identity_action` (`cmd/src/lib.rs:1510`) and its unit tests
  (`:2464-2471`).
- **Emit JSON in the launcher.** In `run_inner`/`run_identity`
  (`crates/qfs/src/identity.rs`), when `json` is set emit a machine-readable object rather
  than prose — credential-free (email + id + a `signed_in`/`resolved` shape; NEVER a password
  hash), matching the `/sys` redaction stance. Human mode is unchanged. Do NOT accept-and-ignore.
- **Correct the clap doc** at `cmd/src/lib.rs:702-706` (and the `IdentityAction` doc at
  `:254-261` if it repeats the stale claim): drop "sign up (email + password)" / "there is
  local sign-up" / "sessions land in t46". State the truth: identity read-back only; sign-up
  moved to `qfs init` (ADR 0008); sessions (t46) shipped and serve the web/OAuth face, not the
  CLI. Keep the §4.1 identity≠authorization framing.

## Policies

**設計 / `workaholic:design`**
- `access-control` / identity≠authorization (§4.1) — the read-back must not imply a grant;
  `whoami` reports who, never what-you-may-do. The corrected help text must keep that line.
- `data-sovereignty` — the JSON answer is credential-free: email + id only, never a password
  hash, matching the `/sys` redaction contract.

**実装 / `workaholic:implementation`**
- `machine-checkable-domain` — a declared `--json` flag that is accepted and silently ignored is
  a lie the type system should have caught; threading it through the action closes the gap.
- `reachability` — a machine-readable identity read-back is what makes the answer consumable by
  an agent without prose-parsing.

**House rules (`CLAUDE.md`)**
- gen-docs anti-drift; every shipped PR bumps the patch (handled at report time).

## Quality Gate

**Acceptance criteria.** `qfs identity whoami --json` emits a valid JSON object (or rejects the
flag with a non-zero exit) — it never accepts-and-emits-prose. `qfs identity --help` no longer
mentions sign-up/signup or "sessions land in t46". Human `whoami` output unchanged.

**Verification method.** Run both commands and read the output; a unit test in `qfs-cmd` pins the
`json` field threading, and a test in `qfs`/`identity.rs` pins JSON emission.

**Gate that must pass.** `cargo fmt -p qfs-cmd -p qfs` before commit. Then `cargo build --workspace`,
`cargo test -p qfs-cmd -p qfs` (or `--workspace`), `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check`, and `cargo run -p xtask -- gen-docs --check` (help text feeds no
generated doc, but run it). Verify by running `qfs identity whoami --json` and
`qfs identity --help` and reading the output.
