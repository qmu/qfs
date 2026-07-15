---
created_at: 2026-07-09T14:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort: 4h
commit_hash: 63be8e2
category: Added
depends_on: [20260709104256-reference-convention-transform-surface.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Column-type refined-name resolution + `create table … OF <name>` — the write-membership boundary

## Overview

The **(C) split of the reference-convention ticket** (`20260709104256`), carved off when that
ticket was driven as (A)+(B): the mechanical locks, the docs, and the *name-form canonicalization*
(`create type <name>`, `of <name>`) landed there, but the two pieces that need a real **resolution
seam** — not just surface canonicalization — were deferred here so they can be designed rather than
rushed. Both are the same underlying gap: **a named refined type used as a type has no resolution
path today.**

Two concrete surfaces, one mechanism:

1. **A column typed by a refined-type name.** Blueprint §5.4 rules that a column's type is a base
   `ColumnType` token *or a declared type name* in **one namespace** (`email email`). The grammar
   already parses it (`table_column_def` reads the type as a bare ident), but nothing **resolves**
   the name: `ColumnType::parse("email")` returns `None`, so a column typed by a refined-type name
   cannot be rehydrated into a `Schema` today. Resolving it means looking the name up in the
   `/type` catalog (`/sys/drivers`, `kind='type'`), taking its **base column type** for the
   structural slot and carrying its **refinement predicate** to the membership boundary.

2. **`create table … OF <name>`.** The write-target `OF` clause (blueprint §5.4's
   `CREATE TABLE /sql/shop/customers OF customer`) is **unbuilt** — `create_table_stmt` requires a
   parenthesised column list and has no `OF` clause (verified 2026-07-09). Building it is what makes
   a *table* carry a named type as its contract, which in turn is the write boundary where a refined
   type's membership is enforced end-to-end. The root ticket (`20260709104254`) shipped
   `check_membership` and wired it at the **declared-view `OF`** delivery boundary as the hermetic
   end-to-end; the **write-side** end-to-end (a `VALUES`/pipeline write into an `OF` table refused
   for a violating row) waits on this `OF` grammar + the schema-resolution seam below.

## The design question (settle before coding)

How does a **named refined type** resolve when used *as a type* (column position, `OF <name>`)?
A column typed `email` must yield, at plan/describe time, both a **structural** `ColumnType` (so
the rest of the type checker and DESCRIBE see a concrete type) and a **refinement** (so membership
runs at the write boundary). Open sub-questions the implementer/Fable must rule:

- **Representation.** Does `Schema`/`ColumnType` grow a `Named(name)` / `Refined { base, name }`
  variant, or does resolution happen *eagerly* (a column typed `email` is stored as its base
  `text` plus a side-table of per-column refinements)? The leaf-crate constraint holds:
  `qfs-types` must stay a leaf — catalog lookup lives in `qfs-core`, not pushed into the type
  vocabulary. (Cross-reference the root chapter §5.2's "`unknown` is a state, not a type" ruling —
  a *named* type is the opposite: a fully-known contract.)
- **Resolution timing.** Declare-time (a `create table … of customer` resolves `customer`'s
  columns+refinement once, at CREATE) vs. read/describe-time (resolve on every describe). The
  declared-view `OF` path already resolves the type's columns at spec-build time
  (`declared_eval::view_specs`); mirror that for tables.
- **Nested/qualified names.** `chatwork/message`, `cloudflare/zone` resolve the same way (the
  qualified-name form (B) already canonicalizes to `/type/<name>`); the resolver keys on the
  canonical `/type/…` catalog path.
- **`unknown` interaction.** A column whose refined type is itself unresolved (unknown name) is a
  declare-time error (a declaration is a contract), consistent with the root chapter.

## Owner ruling (2026-07-09, design session): name resolution — no module system

Settled in the owner design discussion the same day this ticket was carved; the resolver built here
must conform to it. Three rules, shell-style (the skeuomorphism constraint: a person's path
intuition must predict the behavior — and Unix's own answer to a flat namespace at scale is tree +
$PATH + absolute escape hatch, never an import language):

1. **Reference = registry-relative name.** Bare where the catalog is flat (`transform triage`,
   `of customer`), qualified where it nests (`chatwork/message`) — exactly §5.5 as landed. Ambiguity
   is a **structured error, never a silent pick**; a declared name may not shadow a base
   `ColumnType` token (declare-time error, already in §5.4).
2. **Store-time canonicalization.** When a statement is persisted (`CREATE VIEW`/`JOB`/`ENDPOINT`,
   a stored transform reference, provenance/audit records), the stored artifact carries the
   resolved **absolute catalog path** (`/type/chatwork/message`, `/transform/billing/triage`) —
   short names are interactive ergonomics only; stored artifacts are deterministic. qfs can enforce
   mechanically (every statement passes plan-time resolution) what Unix merely recommends
   ("scripts use absolute paths").
3. **No `import`/`open`, no parallel registry — ever.** Lexical imports are
   considered-and-rejected: they break query self-containment (a stored/pasted query would need its
   preamble carried along) and double-borrow a second skeuomorph. The recorded **escape hatch** if
   scale ever demands more ergonomics: a session **search path over catalog prefixes**
   ($PATH-style, interactive-only, canonicalized away by rule 2 at store time) — never a module
   language. Content-hash identity under the name (audit pins the exact definition a run used) is
   noted as the compatible future substrate.

## Key Files

- `packages/qfs/crates/parser/src/grammar.rs` — `create_table_stmt` gains an optional `OF <name>`
  (bare qualified name → `/type/<name>`, the (B) canonicalization already exists to reuse); mutually
  exclusive with the parenthesised column list, or additive (rule it).
- `packages/qfs/crates/types/src/schema.rs` — whether `ColumnType`/`Schema` grows a named/refined
  representation (leaf-crate constraint: no catalog logic here).
- `packages/qfs/crates/core/src/typeck.rs`, `crates/core/src/eval.rs` — resolve a refined-type name
  in column position against the `/type` catalog; carry the refinement to the write boundary.
- `packages/qfs/crates/core/src/membership.rs` — already exists (root ticket); the write-side
  boundary calls it once `OF` tables carry the refinement.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — `DeclaredType` already loads `columns` +
  `refinement`; a table's `OF` resolution reuses it.
- `docs/blueprint.md` §5.4 — promote the write-side membership from "the boundary this ticket wires"
  (declared-view) to the general `OF`-table boundary once built.
- `docs/cookbook/databases.md` — the deferred refined-type **write** recipe (declare → conforming
  `VALUES`/`OF` write passes → violating write refused) lands here, replacing the declare-only
  placeholder the root ticket left (cross-reference: root's cookbook note).

## Related History

- [20260709104254-blueprint-type-system-chapter.md](.workaholic/tickets/archive/work-20260709-023822/20260709104254-blueprint-type-system-chapter.md) — the root chapter: shipped `CREATE TYPE … WHERE`, `check_membership`, and the declared-view `OF` boundary; deferred this write-side seam
- [20260709104256-reference-convention-transform-surface.md] — the (A)+(B) parent: name-form canonicalization for `create type`/`of`, from which this (C) was split

## Implementation Steps

1. Settle the design question above (representation + resolution timing) — a short blueprint §5.4
   addendum or a Fable design brief, since it decides the `Schema` shape. Row it before coding.
2. Add `create table <path> OF <name>` to `create_table_stmt` (reusing (B)'s qualified-name
   canonicalization); desugar to the catalog write carrying the `OF` type reference.
3. Implement column-type refined-name resolution: a column typed by a declared type name resolves
   its base `ColumnType` (structural) + refinement (contract) against the `/type` catalog, in one
   namespace with base tokens; an unknown name is a declare-time error.
4. Wire the **write-side** membership: a `VALUES`/pipeline write into an `OF` table (or a column
   typed by a refined type) membership-checks each candidate row via `check_membership` at the
   eval/commit boundary; a violating row is a structured refusal.
5. Promote the blueprint §5.4 write-boundary prose and add the cookbook refined-type **write**
   recipe (declare → conforming write passes → violating refused), replacing the declare-only
   placeholder; keep the cookbook parse ratchet green.
6. Full anti-drift suite; tick the mission's type-reference-name-ification acceptance (shared with
   the (A)+(B) parent — record which half this completes).

## Quality Gate

**Acceptance criteria:**
- A column typed by a declared refined-type name resolves (structural base type + refinement)
  against the `/type` catalog; an unknown type name is a declare-time structured error.
- `create table <path> OF <name>` parses, resolves the named type, and attaches it as the table's
  contract.
- **Write-side end-to-end**: a conforming `VALUES`/`OF` write passes; a violating one is refused
  with a structured error naming the column/predicate — hermetic tests + a cookbook write recipe.
- Blueprint §5.4 write-boundary prose reflects the built state; `qfs-types` stays a leaf.

**Verification method:**
- `cd packages/qfs && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- `cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` green; the
  refined-type write recipe passes `crates/test/tests/cookbook_skills.rs`.

**Gate:** design question ruled + full suite + write-side membership tests + cookbook ratchet green;
mission acceptance updated.

## Considerations

- Pre-release, experimental: hard breaks are correct; no migration/back-compat for the `Schema`
  representation change (memory: experimental-no-backward-compat).
- Keep type-vocabulary logic in `qfs-types` (leaf) and catalog resolution in `qfs-core` — do not
  push catalog lookups into the leaf and break the acyclic spine.
- This is where the root ticket's deferred write-membership end-to-end is completed; the declared-
  view `OF` boundary (already shipped) and this table `OF` boundary should share `check_membership`.

## Final Report

Development completed as planned. The representation ruling is eager structural resolution: declared
type names stay out of `qfs-types`, `qfs-core` resolves them through an injected pure catalog lookup,
and the qfs binary's SQL apply facet performs the System DB I/O at the commit boundary. `CREATE TABLE
... OF <name>` now desugars through the SQL catalog shape, attaches a persisted table contract, and
`VALUES` writes into that table are checked before the stock SQL applier commits the row.

### Discovered Insights

- **Insight**: SQL contract enforcement belongs in a qfs-side apply facet, not in `qfs-driver-sql`.
  **Context**: the SQL driver is intentionally a leaf and cannot read the `/type` catalog; wrapping
  its applier keeps catalog resolution in the binary composition layer while preserving the existing
  driver boundary.
- **Insight**: Pipeline-sourced writes still do not carry materialized row payloads through
  `EffectInput`.
  **Context**: the new membership guard checks row-bearing `INSERT`/`UPSERT` effects, including
  `VALUES`; if the runtime later materializes pipeline rows at commit time, that boundary should
  pass those rows through the same contract check rather than adding a second membership path.
- **Insight**: Table contract persistence is cross-database relative to the user SQL DDL.
  **Context**: the SQL table is created by the connection database and the qfs contract row is then
  recorded in the System DB; failures after DDL application are surfaced, but this is not an atomic
  two-database transaction.
