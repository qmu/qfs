---
created_at: 2026-07-16T21:41:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort:
commit_hash:
category:
depends_on:
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# Cloud account declarations ship: CREATE ACCOUNT … SECRET '<ref>' with bind-time resolution

## Overview

Mission acceptance item 1 (concern `create-account-ships-the-core-two`, rescoped to the SECRET
edge). A cloud mount should come from committed declarations alone — `CREATE ACCOUNT` +
`CONNECT` — with **no `qfs account add` prerequisite**. `docs/roadmap.md:120` carries this as
🧭 proposed. Unblocked by the 20260716143641 re-homing (consent writes are now
ledger-transactional in the System DB).

The scaffolding exists but is split across two lanes (verified against source this session):

- **The resolver exists and serves §13 mounts only.** `resolve_secret_ref`
  (`secret_ref.rs:79-99`) handles `env:<VAR>` / `vault:<driver>/<connection>` lazily with
  secret-free errors. Declared REST mounts consume it (`commit.rs:350-388` →
  `declared_driver::declared_secrets`, `declared_driver.rs:711-747`); `DeclaredMount` carries
  `secret_ref`.
- **Cloud mounts drop the reference on the floor.** `CloudMount` (`cloud_mounts.rs:17-29`) has
  no `secret_ref` field — `from_bindings` (:66-75) discards the binding row's `secret_ref` — so
  the cloud commit path never reaches the resolver. Cloud binds resolve by **account label**
  only: `networked_credential(driver, connection)` (`commit.rs:786-820`) builds
  `CredentialKey::new(DriverId(driver), ConnectionId(account))`; Google goes through
  `google_stack_for_mount` (`commit.rs:610-647`) → `refresh_token_key(email)`
  (`google-auth/src/source.rs:86-97`). Both require a token previously sealed by
  `qfs account add` (`account.rs:225-228` google, `:293-297` cloud).
- **CREATE ACCOUNT has no SECRET clause.** `create_account_stmt` (`grammar.rs:2364-2375`) parses
  only `APP '<label>'`; `ACCOUNT_COLUMNS = ["provider","account","app"]` (:2328); the
  `/sys/accounts` schema (`driver-sys/src/schema.rs:258-265`) has structurally no secret column.
  A trailing `SECRET '<ref>'` today dies as leftover tokens, not a crisp error.
- **The consent gate stays.** `cloud_bind_allowed` (`commit.rs:727-752`) checks
  `(driver, account)` consent + sign-in; `declare_account` already records consent through the
  shared ledgered writer. The SECRET must resolve *in addition to* consent, never instead of it.

Blueprint §13's rule is "no clause can carry a secret **value**" — a `SECRET '<ref>'`
**reference** (`env:`/`vault:`) is compliant, same as `CONNECT`'s existing clause. The parse-only
trap the mission text warns about ("a surface that cannot resolve") is avoided because this
ticket ships the resolution, not just the clause.

## Implementation Steps

1. **Grammar**: add an optional `SECRET '<ref>'` clause to `create_account_stmt` (mirror
   `conn_secret_clause`; precedent for the combined form at `parser/src/tests.rs:847`). Extend
   `ACCOUNT_COLUMNS`/`account_values` with `secret_ref`. Reject an inline non-`env:`/`vault:`
   value at parse or apply with the resolver's `BadScheme` semantics — a reference, never a
   token.
2. **Registry**: add `secret_ref` to the `/sys/accounts` surface — the `connection_consent`
   home gains the column (new System-DB migration, append-only; the fresh #17 body stays
   frozen), `driver-sys/src/schema.rs` and the `SysNode::Accounts` scan expose it, and
   `record_account`/`declare_account`/`record_account_consent` thread it into the ledgered
   write. `qfs dump`'s accounts section and restore's consent replay carry it (it is a
   reference — selectors-only discipline holds).
3. **Bind-time resolution**: thread `secret_ref` through `CloudMount` (stop dropping it in
   `from_bindings`) and teach the account credential path to fall back to it: when
   `networked_credential` / `google_stack_for_mount` find no sealed vault token for the
   account key, resolve the account's declared `secret_ref` via `resolve_secret_ref` (mount's
   own `secret_ref` first, else the consent row's). `env:` serves CI/agents; `vault:` serves a
   token sealed under any label. Fail closed with the resolver's structured error when the
   reference cannot resolve — a declared account never fake-succeeds.
4. **Consent unchanged**: `CREATE ACCOUNT` keeps recording `(driver, account)` consent through
   the shared writer under the signed-in operator; the bind gate keeps requiring it.
5. **Provisioning decision (record, then implement the chosen side)**: accounts are outside the
   `qfs plan/apply` universe today (`provision/src/state.rs:49-66`, `load.rs:299-309`).
   Recommended: **join the universe** as a consent+reference collection (`SysCollection::
   Accounts`) — the row is selectors + a reference, exactly as portable as a path binding — but
   this is an owner call; if declined, record why in the ticket's final report.
6. **Docs**: `docs/roadmap.md` flips 🧭 → ✅ for cloud account declarations; blueprint §8/§13
   note the account-side SECRET reference. `gen-docs`/`gen-skills` decide what re-renders; the
   qfs-faq/cookbook articles that teach `qfs account add` gain the declaration-first path
   (plugin version bump if a taught surface changes).

## Key Files

- `packages/qfs/crates/parser/src/grammar.rs:2316-2375,3128` — CREATE ACCOUNT parse/desugar.
- `packages/qfs/crates/qfs/src/secret_ref.rs` — the resolver (unchanged, reused).
- `packages/qfs/crates/qfs/src/cloud_mounts.rs:17-75` — CloudMount gains `secret_ref`.
- `packages/qfs/crates/qfs/src/commit.rs:601-820` — mount_connection / google_stack_for_mount /
  networked_credential / cloud_bind_allowed: the bind fallback.
- `packages/qfs/crates/qfs/src/account.rs` + `sys.rs record_account` — the ledgered consent
  writer threading `secret_ref`.
- `packages/qfs/crates/store/src/schema/` — the new append-only System-DB migration.
- `packages/qfs/crates/driver-sys/src/schema.rs:258-265` — the /sys/accounts shape.
- `packages/qfs/crates/provision/src/state.rs`, `crates/qfs/src/provision.rs`, `dump.rs`,
  `restore.rs` — the universe/dump/restore arms (step 5).
- `docs/roadmap.md:60-120`, `docs/blueprint.md:529-547,881-891` — the doc flips.

## Policies

- `workaholic:design` / access control — the consent gate is untouched; a declaration can never
  widen access beyond what a signed-in operator granted.
- `workaholic:implementation` / `type-driven-design` — the reference-not-value rule is enforced
  at the type/parse seam, not by convention.
- `workaholic:implementation` / `persistence` — the new column ships as an append-only
  migration; the ledgered write pattern from 20260716143641 is reused as-is.
- `workaholic:implementation` / `coding-standards` + `test`.

## Quality Gate

1. **The end-to-end declaration works with no `qfs account add`**: a fresh XDG home, a signed-in
   operator, `CREATE ACCOUNT github 'work' SECRET 'env:GH_TOKEN'` + `CONNECT /gh TO github
   ACCOUNT 'work'`, and a bind resolves the credential from the env reference — hermetic (the
   resolver is exercised; no live call).
2. Both-directions: the bind-fallback test written against current code first **fails** (the
   cloud path never consults `secret_ref` today); after, it passes.
3. An unresolvable reference fails closed with the structured resolver error; a sealed vault
   token still wins over the reference (out-of-band sealing keeps working).
4. Parse tests: SECRET clause round-trips through desugar; inline non-reference values get a
   crisp error; `create_account_needs_provider_and_label` still holds.
5. Google arm: `CREATE ACCOUNT google 'you@example.com' APP 'client' SECRET
   'vault:google/you@example.com'` records the three-driver consent and the bind resolves the
   refresh token through the reference when the direct key is absent.
6. Hermetic and isolated: temp `XDG_CONFIG_HOME` everywhere; baseline gates (workspace tests,
   clippy, fmt, gen-docs/gen-skills --check, check-migrations, patch bump).

## Considerations

- Sequencing vs ticket 20260716214200 (sql/git → path_binding): independent; both touch the
  binding/registry area lightly — whichever lands second rebases.
- The `AUTH ACCOUNT '<provider>'` declared-driver clause (blueprint §13) already resolves
  accounts at wire time for declared drivers; this ticket is the *compiled cloud* twin plus the
  declaration surface. Keep the two paths converging on the same `(driver, account)` keys.
- Mission item 7 (declared-secrets adapter carries the OAuth app) builds directly on this
  ticket's threading; do not bundle it here.
