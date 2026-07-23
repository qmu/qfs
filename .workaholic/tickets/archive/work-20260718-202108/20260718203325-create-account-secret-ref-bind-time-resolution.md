---
created_at: 2026-07-18T20:33:25+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: []
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# CREATE ACCOUNT … SECRET '<ref>' with bind-time account-reference resolution

## Overview

Today `create_account_stmt` (`packages/qfs/crates/parser/src/grammar.rs:2359-2377`) has no
way to attach a secret reference to the account it declares, so the credential must be added
out of band with a separate `qfs account add`. This ticket makes the account declaration
self-contained: `CREATE ACCOUNT` gains an optional `SECRET '<ref>'` clause whose reference is
**resolved at use** — lazily at request-build time via `networked_credential`, not sealed into a
vault at declaration time. Re-reading the declaration heals state; rotating the credential is a
plain environment change; an inline non-reference secret is a parse error (references only, never
inlined material).

Concretely:

- Extend `create_account_stmt` (`grammar.rs:2359-2377`) with an optional `SECRET '<ref>'` clause,
  reusing the existing `conn_secret_clause` parser (`grammar.rs:2990-2994`). `ACCOUNT_COLUMNS`
  (~`grammar.rs:2328`) gains a `secret_ref` selector column so the reference is a first-class,
  projectable column of the desugared `/sys/accounts` row.
- Carry the reference through the `/sys/accounts` desugar into the driver-sys applier
  (`packages/qfs/crates/driver-sys/src/applier.rs:69`) and the consent writers
  (`packages/qfs/crates/qfs/src/secret_store.rs:524-541`), persisting it on the System-DB
  `connection_consent` row via a **NEW append-only ALTER migration**. Migration
  `#17 system_config_registry.sql` is FROZEN — this is a fresh migration that only adds the
  column, never an in-place edit of a shipped body.
- Bind-time resolution: `networked_credential` (`packages/qfs/crates/qfs/src/commit.rs:789-820`)
  returns a `Secrets` adapter that, on a vault miss, lazily resolves the consent row's `secret_ref`
  through `crate::secret_ref::resolve_secret_ref`
  (`packages/qfs/crates/qfs/src/secret_ref.rs:79`), mirroring the existing `DeclaredSecretRefStore`
  (`packages/qfs/crates/qfs/src/declared_driver.rs:822-858`).
- End state: `CREATE ACCOUNT cf 'mycf' SECRET 'env:CF_TOKEN'` followed by `CONNECT /cf TO cf …`
  binds and resolves its token with **no** `qfs account add` — `resolve_cf_token`
  (`packages/qfs/crates/qfs/src/cf.rs:125-136`) reads the referenced env at use.
- Flip `docs/roadmap.md:121` 🧭→✅ and regenerate the docs.

## Policies

- implementation/honest-surfaces — no parse-only `SECRET` clause that cannot actually resolve at
  bind time; if the grammar accepts the clause, the binder honours it.
- design/data-sovereignty — secrets are referenced, never inlined; an inline non-reference secret
  is a parse error, and the stored artifact carries only the selector.

## Quality Gate

1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo fmt --all --check`
4. `cargo run -p xtask -- gen-docs --check`
5. `cargo run -p xtask -- check-migrations`
6. Acceptance: parse + desugar carries the `SECRET '<ref>'` reference through to the
   `/sys/accounts` row.
7. Acceptance: an applied declaration records the consent row **and** its `secret_ref` in a single
   System-DB transaction.
8. Acceptance: a `cf`-kind mount resolves its credential at use with no `qfs account add` and no
   sealed vault row.
9. Acceptance: an unresolvable reference fails closed, secret-free (no credential leaked into the
   error).
10. Acceptance: `docs/roadmap.md:121` flips to ✅ and `gen-docs --check` is clean.
11. Verification: hermetic parser tests for the clause; a driver-sys applier unit test for the new
    column; a qfs-crate test using `testenv::HomeGuard` that declares a `cf` account with
    `SECRET 'env:CF_TOKEN'`, asserts `resolve_cf_token` succeeds when the env is set and fails
    closed when it is unset; a migration column-exists test.

## Considerations

- The migration touches the same System-DB schema area as the sql/git path-binding cleanup
  (ticket `20260718203327-…`); land the two schema changes cleanly rather than interleaved.
- `conn_secret_clause` is reused verbatim so the reference grammar stays identical between
  `CONNECT` and `CREATE ACCOUNT`, keeping one reference syntax across the surface.
