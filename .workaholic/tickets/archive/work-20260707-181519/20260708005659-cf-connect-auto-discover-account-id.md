---
created_at: 2026-07-08T00:56:59+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Config]
effort:
commit_hash: aebe6f4
category: Changed
depends_on:
mission:
---

# Auto-discover the Cloudflare account id on `qfs connect` so `--at` is optional

## Overview

The `/cf` migration (commit `b9e1137`) moved Cloudflare onto the account/connect model but made
`qfs connect /cf --driver cf --account <label> --at <cloudflare_account_id>` require the operator to
know and type their Cloudflare **account id**. A Cloudflare API token already scopes which
account(s) it can act on, so the account id is derivable from the stored token — requiring it by
hand is avoidable friction (the live `.env` on this branch carries a token but no account id, which
is exactly the mismatch this removes).

Make `--at` **optional** for the `cf` driver: at connect time, when `--at` is omitted, resolve the
account id from the stored token by calling Cloudflare's `GET /accounts` and persist the resolved id
into the binding's `at_locator`. Keep `--at` as an explicit override (still required to disambiguate
a multi-account token, and usable offline). This also finally lets the deferred live D1/KV probe
from `b9e1137` run through the two-command flow.

**Decisions locked at ticket time (Quality Gate interrogation):**

1. **Multi-account token → fail-closed with a list.** When the token can see more than one account
   and `--at` is omitted, refuse the connect with an actionable error listing the visible account
   ids/names and telling the operator to pass `--at <id>`. Never silently bind the first account.
2. **Resolve at connect time, persist to `at_locator`.** `run_connect` calls `/accounts` once,
   writes the resolved single account id into `path_binding.at_locator`, so `qfs connect --list`
   shows it and no per-boot API call is added on the read/apply hot paths.
3. **Gate includes a live probe.** Approval requires the real D1/KV read-only probe to succeed
   through the `--at`-less flow, not only hermetic tests.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — keep the change inside the
  existing connect (`connection.rs`) and cf composition (`cf.rs`) / driver (`driver-cf`) roots; do
  not add a parallel account-resolution path.
- `workaholic:implementation` / `policies/coding-standards.md` — type-driven, compiler-checkable
  Rust; no stringly account plumbing.
- `workaholic:implementation` / `policies/type-driven-design.md` — model the resolution outcome
  explicitly (resolved-single / ambiguous-many / none-visible) rather than an `Option<String>` that
  loses the "why". Reuse the existing `CloudflareAccountId` newtype (`cf.rs`).
- `workaholic:implementation` / `policies/vendor-neutrality.md` — the `/accounts` call and its DTO
  stay behind the `qfs-driver-cf` boundary; `connection.rs` asks the driver crate for accounts, it
  does not embed Cloudflare API shapes itself.
- `workaholic:implementation` / `policies/test.md` — hermetic `MockCfBackend` coverage for
  single-account resolve, multi-account fail-with-list, and explicit `--at` override, plus one
  opt-in live read probe.
- `workaholic:design` / `policies/defense-in-depth.md` — the token resolves from the vault only at
  the connect boundary to make the `/accounts` call; the account id (non-secret) is the only thing
  persisted, never the token.
- `workaholic:operation` / `policies/ci-cd.md` — hermetic tests stay runnable locally; the live
  Cloudflare probe is clearly separated and opt-in.

## Key Files

- `packages/qfs/crates/qfs/src/connection.rs` — `run_connect` currently UPSERTs the binding without
  resolving anything; add cf-specific account-id auto-resolution when `--at` is omitted, before the
  `db_upsert_binding` call, writing the resolved id into `at`.
- `packages/qfs/crates/qfs/src/cf.rs` — `CloudflareAccountId`, `resolve_cf_token`; add the
  connect-time accounts-resolution helper that turns (token, account label) into a resolved id or a
  typed ambiguity/empty outcome.
- `packages/qfs/crates/driver-cf/src/backend.rs` — `CfBackend` / `HttpApiBackend` gained
  `list_d1_databases` etc. in `b9e1137`; add an accounts probe. Note the current `HttpApiBackend` is
  constructed **with** an account id baked into its URL path, but `GET /accounts` needs none — add a
  token-only accounts call (a constructor/method that does not require an account id) and
  `MockCfBackend` support.
- `packages/qfs/crates/qfs/src/commit.rs` — `networked_credential("cf", label)` /
  `cloud_bind_allowed` are the existing vault-resolution seam `resolve_cf_token` already uses; reuse
  them at connect time (fail closed with an actionable error when the token is absent/refused).
- `docs/guide/cli.md` (`/cf` live configuration) and `docs/cookbook/automation.md` (Cloudflare live
  resources) — update the recipe so `--at` is shown as optional, with a note on when it is required
  (multi-account token). Regenerate skills.

## Related History

The `/cf` account/connect model and the `at_locator`/`CloudflareAccountId` plumbing this ticket
extends were introduced one commit earlier on this same branch; the original env-backed driver it
replaced required `CF_ACCOUNT_ID` explicitly.

- [20260707212907-migrate-cloudflare-to-qfs-query-integration.md](.workaholic/tickets/archive/work-20260707-181519/20260707212907-migrate-cloudflare-to-qfs-query-integration.md) - Migrated `/cf` to account/connect state; added `CloudflareAccountId` from `at_locator`, live resource discovery, and the deferred live-probe concern this ticket closes (direct predecessor).
- [20260630203090-cf-live-d1-kv-queue.md](.workaholic/tickets/archive/work-20260707-045409/20260630203090-cf-live-d1-kv-queue.md) - The env-backed `/cf` driver (`CF_ACCOUNT_ID` required) that the migration replaced.

## Implementation Steps

1. Add a token-only accounts probe to `qfs-driver-cf` (`backend.rs`): a `CfBackend` method (and
   `HttpApiBackend` path) that `GET /accounts` and returns the visible accounts as a typed list of
   `(id, name)`; extend `MockCfBackend` with a settable accounts list and a `RecordedCall` variant.
   Because `HttpApiBackend` currently bakes the account id into its base path, introduce a
   constructor/entry that builds the backend from just the transport + token for this pre-account
   call.
2. In `cf.rs`, add a connect-time resolver: given the account label, resolve the token via
   `crate::commit::networked_credential("cf", label)` + `cloud_bind_allowed`, call the accounts
   probe, and return a typed outcome — exactly one account → its `CloudflareAccountId`; zero → a
   "token sees no account" error; more than one → an "ambiguous, pass --at" error carrying the id
   list. Keep the token in a `Secret`, never logged.
3. In `connection.rs` `run_connect`, when `driver == "cf"` and `at.is_none()`: run the resolver,
   map the typed outcome to either a resolved id passed into `db_upsert_binding` (as `at`) or an
   actionable `Err`. When `--at` IS provided, keep today's behavior unchanged (no network call).
   Leave non-cf drivers untouched.
4. Confirm `qfs connect --list` (via `render_path_bindings`) shows the persisted `at` for a cf mount
   resolved this way — it already renders `at_locator`, so no change beyond the write.
5. Update `docs/guide/cli.md` and `docs/cookbook/automation.md` to show `qfs connect /cf --driver cf
   --account mycf` without `--at`, noting `--at <id>` is required only for a multi-account token.
   Run `cargo run -p xtask -- gen-docs` and `gen-skills`, then `--check` both.
6. Bump the qfs patch version (`packages/qfs/crates/qfs/Cargo.toml`, `0.0.29 → 0.0.30`) and, since
   the taught connect surface changes, bump the plugin `version` in all four fields
   (`plugins/qfs/.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, and both fields in
   `.claude-plugin/marketplace.json`).
7. Add hermetic tests (step below is the gate) and run the full Quality Gate.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `qfs connect /cf --driver cf --account <label>` (no `--at`), with a **single-account** token
  stored under `<label>`, succeeds and persists the discovered account id; `qfs connect --list`
  then prints `/cf` with `at <account-id>` and `account <label>`.
- With a **multi-account** token and no `--at`, the same connect **fails** with a non-zero exit and
  an error that names the visible account ids and instructs the operator to pass `--at <id>`; no
  binding row is written/overwritten for that attempt.
- `qfs connect /cf --driver cf --account <label> --at <id>` still works with **no** `/accounts` call
  (explicit override path unchanged); non-cf drivers' connect behavior is byte-for-byte unchanged.
- No token value appears in argv, `connect --list`, `qfs dump`, restored JSONL, logs, or error text.
- `cargo run -p xtask -- gen-docs --check` and `gen-skills --check` are in sync (run under an
  isolated `HOME`/`XDG_CONFIG_HOME` so unrelated defined paths from a shared config home don't
  register into the driver catalog and read as false drift).

**Verification method** — the commands/tests/probes that prove them:

- Hermetic unit tests with `MockCfBackend`: single-account resolve → id persisted; multi-account →
  typed ambiguity error with the id list; zero-account → typed empty error; explicit `--at` bypasses
  the probe (assert no accounts call recorded).
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` (never piped through head/tail — observe the exit code).
- **Live probe (required):** with the token stored via `qfs account add cf mycf` and the real
  Cloudflare account reachable, run `qfs connect /cf --driver cf --account mycf` (no `--at`), then a
  read-only `qfs run --json "/cf/d1/<db>/<table> |> select ... |> limit 1"` and
  `qfs run --json "/cf/kv/<namespace> |> limit 10"`, both returning without error and without a
  token appearing in output. The external `/accounts` call is made by the qfs binary from the
  vaulted token (not by an operator-supplied account id).

**Gate** — what must pass before approval:

- All hermetic suites green, clippy clean, fmt clean, both gen-docs/gen-skills `--check` in sync.
- The live `--at`-less connect resolves the account id and the D1 + KV read-only probes both
  succeed against the real account, closing the deferred live-probe concern from `b9e1137`.

## Considerations

- The `/accounts` call needs the token but **not** an account id, while `HttpApiBackend` today is
  account-scoped by construction; avoid contorting the account-scoped backend — a small token-only
  entry point is cleaner and keeps the vendor API behind the driver boundary
  (`packages/qfs/crates/driver-cf/src/backend.rs`).
- Account-scoped tokens may reject the generic `/user/tokens/verify` endpoint; `/accounts` is the
  right surface to enumerate reachable accounts (noted in the predecessor ticket's considerations).
- Keep the connect-time network call strictly on the `--at`-omitted cf path so `qfs connect` stays
  offline and side-effect-free for every other driver and for explicit `--at`
  (`packages/qfs/crates/qfs/src/connection.rs`).
- Do not regress the D1 `_cf_` internal-table skip or the JSON projection-order aliases preserved in
  `b9e1137` (`packages/qfs/crates/qfs/src/cf.rs`).
- `.env` is now gitignored on this branch; the live probe reads the token through
  `qfs account add cf` on stdin, never onto argv or into a committed file.
