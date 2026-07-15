---
created_at: 2026-07-08T19:27:30+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort:
commit_hash: fd62866
category: Added
depends_on:
mission:
---

# Add CREATE TRANSFORM definition DDL, storage, and lifecycle

## Overview

First of four dependency-ordered tickets splitting the transform-predicate implementation
(supersedes the deleted mega-ticket `20260708002200-transform-predicate-implementation.md`; the
settled design is the archived brief `20260708002100-transform-predicate-design-brief.md` and
blueprint §15, Decision W). The `|> TRANSFORM <name>` **pipe seam already landed** (`be4df97`:
`PipeOp::Transform(TransformRef)`, refused as `transform_not_executable` downstream). This ticket
builds the **definition half**: the `CREATE TRANSFORM` DDL, its system-DB storage, resolution,
lifecycle surface, auth activation, server-body containment, and the provisioning SoT coverage.

**Discovery correction (2026-07-08, HEAD 24c2269):** no `CREATE TRANSFORM` exists anywhere yet —
`DdlKind` (`crates/parser/src/ast.rs:425`) has no `Transform` variant and the grammar has no arm.
This ticket adds both.

A transform definition is **data** (declare → store → activate lifecycle): it names the input and
output schema (reusing `qfs_types::Schema`/`ColumnType`, never a parallel schema language), the
provider, model, and effort, with secrets **by reference only** (`SECRET 'env:…' | 'vault:…'`),
never inline. The three-mode semantics are a **total function of the declared INPUT shape** and the
derivation function lives here with the definition types (consumed by the plan spine in the next
ticket).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the DDL lives in the existing
  `core/ddl` seam, storage in `crates/store`, SoT arms in `crates/provision`; no new top-level areas.
- `workaholic:implementation` / `policies/coding-standards.md` — typed, compiler-checkable Rust; no
  stringly schema or secret plumbing.
- `workaholic:implementation` / `policies/type-driven-design.md` — the definition is an owned typed
  struct; the mode derivation is a total function over the declared INPUT `Schema` (no "ambiguous
  mode" state is representable).
- `workaholic:implementation` / `policies/persistence.md` — definitions are relational system-DB
  rows behind an append-only, immutable migration (the `check-migrations` ratchet).
- `workaholic:implementation` / `policies/test.md` — hermetic coverage for parse/store/derive/
  containment; no network, no credentials.
- `workaholic:design` — `DESCRIBE /transform/<def>` must be pure and self-explanatory; secret
  resolution is lazy (COMMIT-time), never at describe.

## Key Files

Verified anchors at HEAD `24c2269` (2026-07-08):

- `packages/qfs/crates/parser/src/ast.rs:425` — `DdlKind`: add the `Transform` variant;
  `TransformRef` (`:722-731`) is the existing pipe-side reference.
- `packages/qfs/crates/parser/src/grammar.rs:2040,2128,2289` — the `CREATE CONNECTION`
  contextual-ident DDL pattern to mirror for `CREATE TRANSFORM` (keyword set stays frozen at 39).
- `packages/qfs/crates/core/src/ddl/connections.rs:17-58` — `DeclaredConnection`/`parse_connections`:
  the declare→typed-binding + `SECRET '<ref>'` (env:/vault:) model to reuse.
- `packages/qfs/crates/core/src/ddl/server.rs:341` — `from_server_ddl`, the definition-store-time
  validation site: **refuse any `PipeOp::Transform` inside a stored server-binding body**
  (`CREATE VIEW|ENDPOINT|TRIGGER|JOB|WEBHOOK`) with a structured error — no unattended spend.
- `packages/qfs/crates/store/src/lib.rs:221-383` — `SYSTEM_MIGRATIONS` (last = v15): append the
  transform-definitions table as **v16**; pattern: `crates/store/src/schema/system_drivers.sql`
  (v14, the definition-registry table). `Migration` struct: `crates/store/src/migrate.rs:37`.
- `packages/qfs/crates/qfs/src/describe.rs:137,210` — `register_defined_paths` /
  `compiled_describe_registry`: register the `/transform` describe surface (compiled TYPES only —
  keeps gen-docs deterministic); catalog consumes it at `crates/qfs/src/catalog.rs:103`.
- `packages/qfs/crates/qfs/src/account.rs` — auth activation: provider credentials ride the existing
  account/vault machinery; no new credential path.
- `packages/qfs/crates/provision/src/state.rs:49,124,166` — `SysState` / `SysCollection` /
  `SYS_COLLECTIONS`: add a 5th `Transforms` collection with matching `emit.rs` + `diff.rs` arms
  (blueprint §16 names `CREATE TRANSFORM` as a SoT-joining form at `docs/blueprint.md:784`).
- `packages/qfs/crates/driver/src/lib.rs:358,389` — `ProcSig::irreversible`: `REMOVE TRANSFORM`
  rides the standard irreversible gate.
- `packages/qfs/crates/types/src/schema.rs` — `Schema`/`ColumnType` reused for INPUT/OUTPUT.

## Related History

The design axes are settled — do not re-decide them; build them.

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) — the parent design brief (semantics, seam, Decision-K reversal, safety model).
- Commit `be4df97` — the landed grammar seam: `PipeOp::Transform` as a contextual identifier; pipe
  variant lock grew 18 → 19 (`crates/parser/src/tests.rs:1153`, asserts 19 at `:1295`).
- `docs/blueprint.md:563` (§15 transform, Decision W), `:686` (routing ruling), `:745` (§16
  provisioning SoT), `:714` ("a transform definition is system-DB rows → §16 SoT").

## Implementation Steps

1. **AST + grammar:** add `DdlKind::Transform` and parse `CREATE TRANSFORM <name>` with INPUT/OUTPUT
   schema clauses, `PROVIDER`, `MODEL`, `EFFORT`, and `SECRET '<ref>'` as contextual-ident clauses
   (mirror the `CREATE CONNECTION` arm). `keyword_count_is_frozen` stays 39.
2. **Typed definition + mode derivation:** an owned definition struct reusing `qfs_types::Schema`;
   implement the mode total function — single `bytes` column ⇒ extraction, single `array<struct>`
   column ⇒ relation-wise, everything else (including a single `text` column) ⇒ row-wise; an empty
   `INPUT` or `OUTPUT` is a declare-time structured error.
3. **Storage:** system-DB table via new append-only migration v16 (never edit shipped bodies);
   store the definition as data; resolution API for later tickets (name → definition).
4. **Lifecycle surface:** `ls /transform` lists definitions; `DESCRIBE /transform/<def>` reports
   schemas + derived mode, pure (no network, no secret); `REMOVE TRANSFORM` behind the standard
   irreversible ack. Register the describe surface in `compiled_describe_registry`.
5. **Auth activation:** provider credential add/rotate through `account.rs`/vault; the definition
   resolves its secret lazily at COMMIT (later ticket), never at DESCRIBE; inline secret values are
   a parse/declare error.
6. **Containment:** in `from_server_ddl` (server.rs:341), walk the parsed body and refuse any
   `PipeOp::Transform` inside a stored server-binding body with a structured error.
7. **Provisioning SoT:** add `SysCollection::Transforms` + `SysState.transforms` with `emit`/`diff`
   arms; roundtrip test (emit → load → diff = empty plan).
8. Regenerate docs if the describe registry changes surface output (`gen-docs`), keep all ratchets
   green.

## Quality Gate

Distributed from the parent mega-ticket's developer-authored gate (owner-approved 2026-07-08); plus
the common gate all four split tickets share.

**Acceptance criteria:**

- `CREATE TRANSFORM` parses as a contextual-ident DDL; `keyword_count_is_frozen` still asserts 39;
  the pipe-variant lock still asserts 19.
- The definition stores via a new append-only **v16** system migration;
  `cargo run -p xtask -- check-migrations` passes (no shipped body edited).
- Mode derivation asserted directly as a total function: single `bytes` ⇒ extraction; single
  `array<struct>` ⇒ relation-wise; else (incl. single `text`) ⇒ row-wise; empty INPUT/OUTPUT ⇒
  declare-time structured error (no "ambiguous mode" is representable).
- `ls /transform` lists; `DESCRIBE /transform/<def>` reports schemas + derived mode **without**
  touching network or secret; `REMOVE TRANSFORM` requires the irreversible ack.
- A stored server-binding body containing `transform` is refused at definition-store time with a
  structured error.
- `SECRET` accepts only `env:`/`vault:` references; an inline secret value is rejected; no secret
  appears in any output or error text.
- SoT: transforms emit + diff (roundtrip test green).

**Verification method:**

- Hermetic: `cargo test -p qfs-parser -p qfs-lang -p qfs-core -p qfs-store -p qfs-provision -p qfs`
  (workspace-wide when disk allows), plus `check-migrations`, `gen-docs --check`,
  `gen-skills --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check` (never piped).

**Gate:** all of the above green; no network or credential in any test.

## Considerations

- Sibling tickets consume this one: the plan spine reads the resolved definition + mode
  (`20260708192731`), execution resolves the secret at COMMIT (`20260708192732`), docs/versions
  finish the surface (`20260708192733`). Only this ticket has no `depends_on`.
- Experimental / no backward compat: definitive shapes only — no shims or deprecation staging.
- Reuse, don't reinvent: schema = `qfs_types`; lifecycle = "a definition is data" (the declared
  drivers/v14 pattern); secrets = the existing vault seam (`crates/qfs/src/account.rs`).
- Commit via `workaholic:commit` `commit.sh` with explicit file args; never `git add -A`
  (shared-tree concurrent sessions).
- `cargo fmt --check` must not be piped through head/tail (masks the exit code); clippy is
  `--workspace --all-targets -D warnings`, not `--all-features`.
- Shared build host: if `cargo test --workspace` dies with os error 28 (disk), fall back to the
  per-crate list above (`.claude` memory: shared-host-disk-full-cargo-test).

## Implementation Notes (2026-07-09, delivered)

Delivered with two deliberate, coherence-driven deviations from the literal Key Files (all
acceptance criteria met; all four ratchets + clippy + fmt green):

1. **No `DdlKind::Transform` variant.** `CREATE TRANSFORM` carries `INPUT (…)`/`OUTPUT (…)`
   column-list clauses, which the generic `server_ddl` clause loop cannot parse — so, exactly like
   `CREATE TABLE`/`CREATE DRIVER`, it is a **dedicated parser** (`create_transform_stmt`) that
   **desugars in the parser to `INSERT INTO /transform`** (and `REMOVE TRANSFORM <name>` →
   `REMOVE /transform WHERE name == '<name>'`). This is the true "a definition is data" precedent
   (§13 declared drivers → `/sys/drivers`); the `ServerDdl`/`DdlKind` route is for `/server`
   bindings, which `from_server_ddl` would reject anyway. Adding an unused `DdlKind::Transform` that
   the containment/validation path never uses would be dead scaffolding. Keyword count stays 39,
   pipe-variant lock stays 19.
2. **`/transform` is a new top-level driver crate `qfs-driver-transform`** (mirroring
   `qfs-driver-sys`: pure describe facet + injected `TransformBackend`, binary-side impl in
   `crates/qfs/src/transform.rs` over `sys_transforms`). The mode function `derive_mode` +
   `ColumnType::parse` live in **`qfs-types`** (the leaf — reachable by `pushdown` for T2), and the
   owned `TransformDef` (+ validation) in **`qfs-core::ddl::transform`** per the core/ddl seam.

Other notes: v16 migration `sys_transforms`; the derived cardinality **mode is never stored** (a
pure function of `input`, spliced in on scan); `secret_ref` is stored/listed as a reference only.
The SoT `SysCollection::Transforms` maps to the **top-level `/transform`** (not `/sys/transforms`) —
`path()`/emit/load handle it as a first-segment `transform` collection. **Model/provider/effort
values with non-ident chars (e.g. `claude-sonnet-5`) must be quoted** (`MODEL 'claude-sonnet-5'`) —
`-` is not a lexer token. `docs/drivers.md` regenerated (the compiled catalog now lists
`/transform`); the taught language/cookbook surface is T4's job.
