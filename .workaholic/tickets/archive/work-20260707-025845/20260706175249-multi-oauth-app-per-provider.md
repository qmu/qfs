---
created_at: 2026-07-06T17:52:49+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, DB, Domain]
effort: 4h
commit_hash: f1557fd
category: Added
depends_on:
---

# Multiple OAuth apps per provider (per-org credentials, keyed by label)

## Overview

Today `qfs app add <provider>` holds **one OAuth app per provider**. That makes qfs single-tenant for
Google: every account authorizes through the *same* `credentials.json`, so an account in a **different
Google Workspace organization** than the app's owning org is refused at consent with
`403 org_internal` ("組織内でのみ利用可能"). An operator who works across their own org and a client's
org (a real, recurring situation) cannot connect the client's Drive/Gmail at all without destroying
their own app registration.

Let qfs register **several OAuth apps per provider, each under a label** (an org key), and let an
account authorize + refresh against **its own org's app**. Then a@… authorizes through the home-org
app and a client account authorizes through the client-org's own (Internal-consent) app — no
`org_internal`, no external-verification detour, durable tokens for each.

**Headline finding (from source map):** this is *not* a storage-schema limitation. The app is an
ordinary envelope-encrypted `secret_store` row whose primary key is already `(driver, connection)` —
i.e. `("google-app", <any-label>)` is representable today. The single-slot limit is **two hardcoded
`"default"` literals** plus the absence of any account→app mapping. So the work is: (1) let
`app add`/`app_key` carry a label instead of `"default"`; (2) persist which app an account authorized
against; (3) thread that label through the consent + refresh resolution so each account uses its own
client credentials.

This is **experimental, hard-break** work: no backward-compat/migration shim for the existing
single `"default"` app — after this lands the operator simply re-registers apps under labels (see
[[experimental-no-backward-compat]]). The one required schema change ships as a normal forward-only
**migration #12** (the sanctioned mechanism, not a compat concern).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — changes stay in the crates that
  already own this seam (`crates/qfs`, `crates/store`, `crates/cmd`, `crates/parser`); no new ad-hoc
  location (applies to all code work).
- `workaholic:implementation` / `policies/coding-standards.md` — replace the stringly-hardcoded
  `"default"` chokepoints with a typed app-label selector threaded through the API, so the compiler
  catches missing call sites rather than a runtime fallback silently reusing one app (applies to all
  code work; the Domain lens's type-driven design).
- `workaholic:implementation` / `policies/test.md` — the existing app/account round-trip and
  migration-shape tests write down real behavior against the real store; every one that asserts the
  `"default"` key or the migration count must be updated to the new model, and new tests must cover
  two coexisting apps + correct per-account resolution.
- `workaholic:operation` / `policies/ci-cd.md` — the schema change ships as migration #12 under the
  checksum-verified, forward-only mechanism guarded by `cargo run -p xtask -- check-migrations`; the
  shipped body of `project_secrets.sql` must NOT be edited (frozen) — add a new migration instead.
- `workaholic:implementation` — persistence/DB lens (relational-first, domain↔persistence
  segregation): the account→app binding is a persisted relation; keep the resolution logic in the
  domain layer and the row in the store layer, not smeared across `commit.rs`. (Open the pillar index
  for the exact persistence policy.)
- `workaholic:design` — security/least-privilege lens: each account authorizes and refreshes against
  **only** its own org's client credentials; a token minted under org A's app must never be refreshed
  under org B's app. Treat the app-label as part of the credential identity so cross-org bleed is
  structurally impossible. (Open the design pillar index for the security-design policy.)

## Key Files

Current single-slot chokepoints and the exact sites to change (from the source map):

- `packages/qfs/crates/qfs/src/google.rs:74-76` — `GOOGLE_APP_DRIVER = "google-app"`,
  `GOOGLE_APP_CONNECTION = "default"` (the hardcoded label constant).
- `packages/qfs/crates/qfs/src/account.rs:124-137` — `app_key`: hardcodes `ConnectionId::new("default")`
  and rejects non-`google`; the **write-side chokepoint**. `app_add` (140-154), `app_list` (157-173,
  must also surface the label), `app_remove` (174-186).
- `packages/qfs/crates/qfs/src/google.rs:82-104` — `google_app_config` / `app_config_from_store`: the
  **read-side chokepoint** (builds `CredentialKey(DriverId("google-app"), ConnectionId("default"))`).
- `packages/qfs/crates/qfs/src/google.rs:207-221` — `google_stack_for_account(email)`: the account
  email keys only the refresh token, never the app — must gain an app-label parameter.
- `packages/qfs/crates/qfs/src/google.rs:240-261` — `run_google_consent`: the live authorize flow;
  must know which org's app to consent through.
- `packages/qfs/crates/qfs/src/account.rs:193-243` — `add_google` / `record_google_consents`: consent
  time; where the account→app binding is captured.
- `packages/qfs/crates/qfs/src/commit.rs:582-606` — `google_stack_for_mount`: pulls `mount.account`
  (587) → `google_stack_for_account` (605); the **per-mount refresh site** where the app label is
  resolved.
- `packages/qfs/crates/store/src/schema/project_secrets.sql:11-18` — `secret_store(driver, connection,
  …, PRIMARY KEY (driver, connection))`; **already admits multiple app labels** — do not edit (frozen
  v2).
- `packages/qfs/crates/store/src/schema/project_mount_coordinate.sql:17-18` — `path_binding` gained
  `host` + `account`; the model to mirror if the app selector is stored on the mount (a new
  `app` column via migration #12).
- `packages/qfs/crates/store/src/lib.rs:377-501` — `PROJECT_MIGRATIONS` (head **v11**); append the new
  `Migration { version: 12, … }` here (501) with a new `schema/project_*.sql`.
- `packages/qfs/crates/store/src/migrate.rs` — the versioned, checksum-verified, forward-only engine.
- `packages/qfs/crates/cmd/src/lib.rs:634-661` — clap `AppVerb::Add { provider }` (add a label arg) and
  `AccountVerb::Add { provider, label }`; `Command::Connect { … account … }` (457-486, an `--app`
  beside `--account` at 476-478); DTOs `ConnectionAction::Connect` (113-147), `AccountAction` (~218).
- `packages/qfs/crates/parser/src/grammar.rs` — `create_account_stmt` (1667-1677) + `ACCOUNT_COLUMNS`
  (1635); `CONNECT` tail `connect_secret_clauses` (1757-1797) + `conn_account_clause` (2283-2286) as
  the template for a new `APP '<label>'` clause. The in-language twins must move with the CLI.
- `packages/qfs/crates/google-auth/src/source.rs:38-97` — `refresh_token_key` (account email →
  token); stays account-keyed (the app dimension is on the *app config* lookup, not the token key).

## Related History

- [20260703040000-create-account-language-surface.md](.workaholic/tickets/archive/work-20260705-173620/20260703040000-create-account-language-surface.md) — added `CREATE ACCOUNT` and `/sys/accounts`; the account/consent surface this feature extends with an app dimension.
- Defined-paths / CONNECT epic (`path_binding` registry, `--account`/`--host` on `qfs connect`) — the mount-carries-the-credential model this mirrors for the app selector. See [[defined-paths-registration-model]].
- [20260706163521-qfs-faq-reference-skill.md](.workaholic/tickets/todo/a-qmu-jp/20260706163521-qfs-faq-reference-skill.md) — the FAQ documents the *current* one-app-per-provider limit + the `org_internal` workaround; **when this feature lands, that FAQ entry must be updated** (the anti-drift model working as intended). Cross-reference, not a dependency.

## Implementation Steps

1. **Label the app registration.** Replace the hardcoded `"default"` (`account.rs:132`,
   `google.rs:76`) with an app-label carried through `app_key(provider, label)` and add the label arg
   to `AppVerb::Add` (`cmd/src/lib.rs:634`). `qfs app add google <org-label>` now stores
   `("google-app", "<org-label>")`; `app list` prints provider + **label** + created_at; `app remove`
   takes the label. Decide the label ergonomics (required vs. an explicit default) — recommend
   **required label** for clarity (hard break; no implicit slot).
2. **Bind an account to its app at authorization.** `qfs account add google <email> --app <org-label>`
   (+ the `CREATE ACCOUNT … APP '<label>'` twin): `run_google_consent`/`add_google`
   (`google.rs:240`, `account.rs:193`) run the consent through that app's client credentials, and the
   **account→app mapping is persisted** so a later refresh is self-contained. Choose where the mapping
   lives (core design decision):
   - **(a) on the account/consent record** — refresh is fully self-contained from the email; recommend
     this as the primary (consent happens before any mount exists).
   - **(b) `path_binding.app` column on the mount** (migration #12, mirroring `account`) — lets a mount
     override which app services it; add as the *selector at connect time* on top of (a).
   Recommended: persist (a) at authorization, and add (b) as an optional per-mount override.
3. **Thread the label through resolution.** `google_app_config`/`app_config_from_store`
   (`google.rs:82-104`) take the app label instead of `"default"`; `google_stack_for_account` gains
   the label; `google_stack_for_mount` (`commit.rs:585-605`) derives the label from the account (a) /
   mount (b) and passes it down. Env fallback (`QFS_GOOGLE_CLIENT_ID/SECRET`) maps to a reserved label.
4. **CLI + grammar twins.** `--app` on `qfs connect` (`cmd/src/lib.rs:476`) and the `APP '<label>'`
   contextual-ident clause on `CONNECT` (`grammar.rs:1757-1797,2283`); the app selector on
   `CREATE ACCOUNT` columns (`grammar.rs:1635-1677`). Every runnable `qfs`/`CONNECT`/`CREATE ACCOUNT`
   example must parse (the cookbook ratchet).
5. **Migration #12** (only if (b) or any persisted mapping needs a column): new
   `schema/project_<name>.sql` + a `Migration { version: 12, … }` appended at `store/src/lib.rs:501`;
   never edit a frozen shipped body; `check-migrations` must pass.
6. **Tests.** Update the `"default"`-key assertions and migration-count/checksum fixtures
   (`account.rs:633-667`, `google.rs:267-323`, `store/src/lib.rs:647-717,826-859`,
   `secret_store.rs:1063-1077`, `cmd/tests/e2e_cli.rs:728-796,825,909-911`, `google-auth/src/tests.rs`).
   Add coverage for **two coexisting apps** and **correct per-account app resolution** (the core new
   guarantee).
7. **Docs + plugin.** Update the FAQ entry (once it exists) and any connect guide; bump the qfs patch
   version and — since the taught connect/account surface changes — the four plugin `version` fields.

## Quality Gate

**Acceptance criteria:**

- Two Google apps can be registered under distinct labels simultaneously (`qfs app add google home`
  then `qfs app add google client`), and `qfs app list` shows both with their labels — the second no
  longer clobbers the first.
- An account authorized `--app client` runs consent through the client app's client_id and, on a later
  read/commit, refreshes through the **same** client app — never the home app (the anti-cross-org
  guarantee), verified by the resolved client_id at `google_app_config`.
- A cross-org account that previously failed `org_internal` under the shared app can be authorized
  under a client-org-issued app (validated at the flow level with a stubbed client config; live Google
  auth is out of scope for hermetic tests).
- `create ACCOUNT … APP '…'` and `connect … --app …` parse and desugar to the same `/sys` effects as
  their CLI twins.
- No frozen migration body is edited; any schema change is migration #12 and `check-migrations` is
  green.

**Verification method:**

- `cargo test --workspace` green — including updated `account.rs`/`google.rs` unit tests, the new
  two-app/resolution tests, `store` migration-shape tests, and `cmd/tests/e2e_cli.rs`.
- `cargo run -p xtask -- check-migrations` exits 0 (no edited shipped bodies; contiguous to v12).
- `cargo run -p xtask -- gen-docs --check` and `gen-skills --check` exit 0 (grammar/driver docs +
  skills regenerated for the new `--app` / `APP` surface).
- `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`.
- Manual/flow-level: a stubbed two-org config proves account→app resolution picks the right client_id.

**Gate:** all of the above green, with an explicit test demonstrating that an account bound to app A
never resolves app B, before approval.

## Considerations

- **Security — no cross-org token bleed.** The app-label is part of the credential identity; a refresh
  must fail closed rather than silently fall back to another app if the bound app is missing
  (`google.rs:82-104`). Do not let the env-var fallback mask a missing labeled app.
- **Hard break, no shim.** The existing single `"default"` app is abandoned; the operator re-registers
  under labels. No migration of the old row, no deprecation window (experimental; see
  [[experimental-no-backward-compat]], [[no-risk-framing-for-experimental]]).
- **Where the account→app binding lives is the load-bearing design choice** — resolve step 2(a) vs
  (b) before coding; (a) keeps refresh self-contained, (b) adds per-mount flexibility. Decide at
  `/drive` approval.
- **`oauth_store.rs` is unrelated** — that is qfs's server-side RFC 7591 client registry, not the
  Google app registration. Don't touch it for this.
- **FAQ coupling** — `20260706163521-qfs-faq-reference-skill.md` states the current limit; landing
  this makes that entry stale, so update it in the same PR that ships this (keeps the FAQ verified-true).
- **Consent timing** — consent runs at `account add`, before any mount, so the app must be selectable
  there; a mount-only (`path_binding.app`) design alone is insufficient (`account.rs:193`,
  `commit.rs:585`).

## Final Report

Implemented labeled OAuth app support for Google. App credentials are now stored as
`google-app/<label>`, `app list`/`app remove` include labels, Google account authorization/import
requires `--app`, `CREATE ACCOUNT` supports `APP '<label>'`, `qfs connect` supports `--app`, and
`CONNECT` supports `APP '<label>'`.

Added project migration #12 for `connection_consent.app` and `path_binding.app`. Commit-time Google
stack resolution now uses `path_binding.app` first, then the bound account consent app, and fails
closed if no app label is available. `/sys/paths`, `/sys/accounts`, dump, and restore all carry the
new selector.

Verification passed:

- `cargo fmt --all --check`
- `cargo test -p qfs-cmd`
- `cargo test -p qfs-parser`
- `cargo test -p qfs-store`
- `cargo test -p qfs`
- `cargo run -p xtask -- check-migrations`
- `cargo run -p xtask -- gen-docs --check`
- `cargo run -p xtask -- gen-skills --check`
