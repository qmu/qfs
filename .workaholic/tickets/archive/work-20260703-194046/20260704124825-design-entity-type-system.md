---
created_at: 2026-07-04T12:48:25+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort: 2h
commit_hash: 00225bf
category: Changed
depends_on:
---

# Design blueprint: types are sets — the entity type system qfs already contains

## Overview

Design — **as the comprehensive blueprint, not another numbered ADR** — the strict entity type
system the owner asked for: plgg-disciplined, Prisma-shaped ("define entity to store rdb"), with
modal-logic formal verification expected to attach — **without adding a type system**.

**The deliverable form is itself a decision (owner directive, 2026-07-04):** a pile of historical
ADR snapshots records the scrap-and-build churn, which is meaningless to hold — git holds history.
What is worth holding is **one comprehensive design snapshot: the blueprint** — the current
intended design of the whole language, *including not-yet-implemented parts, blueprint first* —
revised in place. This ticket therefore produces (a) the blueprint document with the type system
as its newest chapter, and (b) the fold-and-retire plan for the existing `docs/adr/0001–0009`
pile (whatever is still true folds into the blueprint; the numbers stop growing).

The essential observation the ADR must be built on:

> RFD §2: *a path is a query that resolves to a set.* In type theory, *a type is a set of values.*
> Therefore qfs's query language **is already a type language**: `WHERE` is refinement, `EXCEPT` is
> set difference, membership is the consistency check. Nothing second needs inventing — no external
> schema DSL (Prisma's move), no validator runtime (plgg's move), no new column-type vocabulary.

One new concept completes it — everything else is composition of what exists:

**An entity type is a named, path-addressed, *intensional* relation**: a schema plus an optional
refining predicate, declared in the definition layer (ADR 0009 rev. 2), describable but not
enumerable. A table is an *extensional* relation (stored rows) constrained to be a subset of its
type. plgg's mapping is exact: Box tag ≙ the path (nominality), `cast`/`forProp` composition ≙
predicate conjunction over a schema, `decodeRow` ≙ membership at the read boundary.

Sketch of the surface (the ADR owns the final shape; zero new frozen keywords — `TYPE`/`OF` are
contextual idents, `WHERE` is already core):

```sql
CREATE TYPE /type/email (value text NOT NULL) WHERE value LIKE '%@%'

CREATE TYPE /type/customer (
  id int PRIMARY KEY,
  email /type/email UNIQUE,
  joined timestamp NOT NULL
)

CREATE TABLE /sql/shop/customers OF /type/customer
```

**The decision points the blueprint must settle** (each with rationale and rejected alternatives):

0. **The blueprint itself** — its home (e.g. `docs/blueprint.md` or a `docs/blueprint/` chapter
   tree; it subsumes the design role of RFD-0001 and the ADR pile), its revision discipline
   (in-place edits, git as the only history, implemented-vs-blueprint status marked per section so
   objective-documentation honesty holds), and the fold-and-retire plan for `docs/adr/0001–0009`
   and the VitePress sidebar. Less is better applies to documents too: one design artifact.

1. **The type literal** — recognize that `CREATE TABLE`'s column list already IS an anonymous type
   literal, and factor the grammar accordingly: one production `(<col> <type> …) [WHERE <pred>]`
   used by `CREATE TYPE` (which names it at a path) and `CREATE TABLE` (inline, or by reference
   via `OF <type-path>`). The shipped table grammar becomes a special case, not a sibling feature.
2. **One namespace** — nominality and composition come from the path tree, not a parallel type
   registry: a column's type may be a base `ColumnType` name *or a type path* (`/type/email` — a
   refined text). "Narrow atomics" (email, uuid, non-empty …) are therefore **user-defined refined
   types, not new core variants**: `ColumnType` does not grow. Decide where type nodes mount
   (`/type` vs `/sys/types`), how they resolve at plan time, and what DESCRIBE shows (`ls /type` is
   SHOW TYPES for free).
3. **The consistency contract as set membership** — (a) a literal `VALUES` write into an `OF` table
   is membership-checked at eval time (pure; a structured error names the failing predicate/column);
   (b) a pipeline-sourced write is checked per row at apply (honest — the rows exist only then);
   (c) reads decode by the same membership; (d) **drift is set difference**: the introspected live
   catalog reconciled against the declared type, surfaced structurally in DESCRIBE — the check must
   stay honest about tables mutated outside qfs.
4. **Redefinition** — no migration subsystem (less; qfs is experimental, hard breaks are correct).
   Redefining a type previews a derived reconciliation plan for attached tables; anything
   destructive rides the existing irreversible gate. dbmate/Prisma-migrate machinery is explicitly
   rejected for now, with the reasoning recorded.
5. **Verification-readiness as a representation constraint** — build no verifier. State the
   intended model: qfs already forms a Kripke structure (Worlds = database states; committed Plans
   = accessibility), type safety is □(table ⊆ type), and invariants are ordinary predicates in the
   one predicate language. The only obligation *now*: a type is declarative data (schema +
   predicate), never opaque executable code — that alone keeps the model checker's door open.
6. **The driver contract re-founded on types — third-party drivers included.** RFD §5 has every
   driver declare per-node "archetype + schema"; the blueprint lifts that schema surface onto the
   one type system: a driver's output nodes are declared **as types in the one namespace** (the
   path tree), so `DESCRIBE` reports type references, a third-party driver's output rows are
   typed by the same membership check as a table, and an entity type can be shared between a
   table and a driver surface (the same `/type/customer` typing a `/sql` table and a REST
   driver's response). Decide the declaration seam for third-party drivers (how an out-of-tree
   driver registers its types — the registry story is already "paths, functions, codecs"; types
   ride the path registry) and what conformance means for a driver the binary does not compile
   (declared type vs delivered rows = the same drift reconciliation as §3, honestly surfaced).
7. **What is deliberately absent** — record the rejections: an external schema file (the language
   is the schema language), a CALL-based validation surface, new `ColumnType` atomics, a type
   registry beside the path tree, migration tooling, full cross-driver *enforcement* in v1 (the
   contract above declares and reconciles; blocking enforcement ships for `/sql` attachment
   first). Note the plgg bridge (a qfs type rendering a plgg validator, one entity definition
   serving both stacks) as a consequence, not a commitment.

**Boundary:** design only. This ticket produces the blueprint (with the type system chapter and
the ADR fold-and-retire plan) plus its sidebar entry. Implementation tickets are cut from the
accepted blueprint; nothing else is implemented under this ticket.

## Policies

The standard engineering policies that govern this ticket. The implementing session MUST read each
linked policy hard copy before writing the ADR and keep every decision defensible against that
policy's Goal, Responsibility, and Practices.

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/type-driven-design.md` — the subject itself: narrow types to the domain's actual shape, introduced selectively, so expressing a requirement doubles as a consistency check
- `workaholic:implementation` / `policies/persistence.md` — schema-first: the type is the schema's single source; attached tables stay properly normalized relations, never entity-blob dumps
- `workaholic:implementation` / `policies/functional-programming.md` — types stay declarative data; the membership check is pure; no opaque validators
- `workaholic:planning` / `policies/modeling-centric-design.md` — entities/invariants/persistence decided as one model before code
- `workaholic:planning` / `policies/terminology.md` — "type", "table", "schema", "catalog" get one word per concept before the grammar names them
- `workaholic:design` / `policies/vendor-neutrality.md` — Prisma/plgg are inspirations; no second DSL, no runtime dependency, qfs-native surface only
- `workaholic:implementation` / `policies/objective-documentation.md` — a blueprint holds intended design, which this policy forbids passing off as fact: every section carries an explicit implemented-vs-blueprint status so the document stays verifiable

## Key Files

- `packages/qfs/crates/types/src/schema.rs` - `ColumnType`/`Schema` — the structural layer that must NOT grow; type paths resolve onto it
- `packages/qfs/crates/parser/src/grammar.rs` - `create_table_stmt` + `table_column_def` — the column grammar to refactor into the shared type literal
- `packages/qfs/crates/parser/src/ast.rs` - the definition-layer AST and keyword-freeze constraints
- `packages/qfs/crates/core/src/eval.rs` - `values_row_batch` / the parse-time gate — where literal-write membership checks hook
- `packages/qfs/crates/driver-sql/src/conn.rs` - catalog introspection/refresh — the extensional side of the drift reconciliation
- `packages/qfs/crates/qfs/src/connections_config.rs` - where declarations persist today (the CREATE CONNECTION precedent)
- `docs/adr/0009-sql-provisioning-and-ddl-semantics.md` - rev. 2 definition layer this ADR extends
- The plgg project's `README.md` (sibling repo, not vendored here) - Box/cast/decodeRow — the discipline whose mapping onto sets the ADR records

## Related History

The definition layer grew this branch: ADR 0009 rev. 2 made TABLE first-class after the owner
rejected raw catalog DML; this ADR lifts the same layer to its foundation — the type literal — and
names the identity the language always had (paths = sets = types).

- [20260704001232-design-sqlite-dbms-semantics.md](.workaholic/tickets/archive/work-20260703-194046/20260704001232-design-sqlite-dbms-semantics.md) - ADR 0009: definition layer + catalog plumbing
- [20260704001233-implement-sqlite-dbms-management.md](.workaholic/tickets/todo/a-qmu-jp/20260704001233-implement-sqlite-dbms-management.md) - CREATE TABLE implementation whose column grammar becomes the type literal
- [20260630004110-design-connection-declaration-grammar.md](.workaholic/tickets/archive/work-20260629-110121/20260630004110-design-connection-declaration-grammar.md) - the declaration-persistence precedent

## Implementation Steps

1. Decide the blueprint's home and revision discipline (decision point 0), then inventory
   RFD-0001 and docs/adr/0001–0009: what is still true folds into the blueprint's chapters, the
   rest is retired — git keeps the history, the tree keeps one design artifact.
2. Re-read RFD-0001 §2/§3/§4/§5, ADR 0009 rev. 2, and the plgg READMEs; write the type chapter's
   foundation as the sets identity (path = set = type) and the exact plgg↔qfs mapping
   (tag↔path, cast↔predicate, decodeRow↔membership).
3. Draft the blueprint settling decision points 0–7, biased throughout by *less is better*: every
   candidate addition must first fail to be expressible as composition of what exists. Mark each
   section implemented vs blueprint (objective-documentation honesty).
4. Write the example statements the cookbook will later teach (the sketch above, a rejected
   ill-typed INSERT, a drift-detection DESCRIBE, a third-party driver output typed by a shared
   entity type) and check each against the grammar constraints (contextual idents only; zero new
   frozen keywords; the type literal shared with CREATE TABLE).
5. Specify the membership check's placement and error shape per write form (literal vs pipeline),
   the drift reconciliation's honesty conditions, and the driver-conformance reconciliation
   (declared type vs delivered rows) they share.
6. Record the verification model (Kripke structure already present; □(table ⊆ type); invariants as
   ordinary predicates) and the single representation obligation it imposes now.
7. Update the VitePress sidebar (blueprint in, retired ADRs out); cut implementation tickets from
   the accepted blueprint.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- The blueprint exists at its decided home, is in the VitePress sidebar, marks every section
  implemented vs blueprint, and decides all eight points (0–7) with rationale and explicit
  rejections. The ADR fold-and-retire plan is executed or explicitly staged (nothing left
  half-duplicated between the pile and the blueprint).
- Every example statement parses on the shipped grammar or is marked as a proposed additive form
  citing the contextual-ident / zero-new-frozen-keyword rule; the type literal is demonstrably the
  same production `CREATE TABLE` uses; the driver-contract chapter types a third-party output with
  the same mechanism that types a table.
- `ColumnType` and the frozen keyword set are unchanged by the design; the blueprint is consistent
  with (and absorbs) RFD-0001 §2/§3/§4/§5 and ADR 0009 rev. 2.
- The verification story is future-labeled, with the declarative-data representation obligation
  stated; no verifier, no implementation.
- `cargo test --workspace` remains green (no product code changes).

**Verification method:**

- `cd packages/qfs && cargo test --workspace`; `gen-docs --check`; `gen-skills --check` (all
  unchanged/green).
- Parse-check currently-valid examples against the binary; proposed forms against the blueprint's
  grammar-constraint checklist; cross-read against RFD-0001, ADR 0009 rev. 2, and the plgg
  READMEs.

**Gate:**

- All eight decision points decided, the less-is-better bias visibly applied (the rejections
  section is substantive; the document count went *down*, not up), workspace green, and the owner
  approves the blueprint content at `/drive`. This is owner-taste-heavy language design —
  **never auto-approve it in night mode**.

## Considerations

- The keyword freeze binds: `TYPE`/`OF` are contextual idents; the refinement clause reuses the
  frozen `WHERE` and the one predicate language (`packages/qfs/crates/parser/src/ast.rs`)
- An intensional node needs no new archetype if capabilities already express "describable,
  non-enumerable" (empty verb set) — prefer that over enum growth (`packages/qfs/crates/driver/src/lib.rs`)
- Membership on pipeline-sourced writes is inherently apply-time; the ADR must keep the honesty
  split (pure eval check for literals, per-row apply check for pipelines) explicit — never wrong
  rows, never a fake static guarantee
- The type layer is service-neutral by construction; resist the temptation to ship cross-driver
  enforcement in v1 — note it as the natural extension instead
- qfs is experimental: the type-declaration persistence format may hard-break freely while the
  design settles; no compat machinery
- The ADR fold-and-retire must not orphan references: ADR 0008/0009 are cited by tickets, commit
  messages, and code comments — retiring a file means the blueprint absorbs its content and the
  sidebar changes, while git history keeps the citations resolvable (`docs/adr/`, `docs/.vitepress/config.mts`)
- The driver-contract chapter binds RFD §5's per-node schema to the type namespace; the Driver
  trait's `describe` surface is where a declared-type reference would eventually ride — design the
  seam so an out-of-tree driver declares types without compiling into the binary
  (`packages/qfs/crates/driver/src/lib.rs`)

## Final Report

Development completed as planned. Delivered `docs/blueprint.md` (commit `1d34424`) — the one
living design document, 12 chapters, per-section implemented/blueprint/parked status — with the
types-are-sets type system and the driver-contract re-founding as its blueprint chapters, all
eight decision points (0–7) settled with recorded rejections. The ADR pile `docs/adr/0001–0009`
was deleted (absorbed; git holds history; −1,201 lines against +358), RFD-0001 was banner-marked
as a superseded frozen citation anchor, and the mechanical citation sweep was cut as ticket
`20260704140352`. Owner approved the blueprint content at the gate.

### Discovered Insights

- **Insight**: `CREATE POLICY` already parses `ALLOW <verbs> [ON <driver>] [FOR <subject>]
  [AT <path-glob>] [WHERE <cond>]` (t57) — the path-scope language surface for fine-grained
  authorization exists; only the runtime `CapabilitySet` ignores paths.
  **Context**: This collapsed the sibling path-aware-capability ticket (20260704110923) from a
  design+grammar effort to a pure enforcement change (an additive `Option<PathScope>` on the
  grant tuple). Checking what the grammar already parses before designing new surface is the
  cheapest design move in this codebase.
- **Insight**: The blueprint reframing turned the shipped `CREATE TABLE (col type …)` column list
  into the anonymous form of the §5 type literal — the feature shipped before its concept was
  named, and no code needs to move for the concept to land.
  **Context**: Future `CREATE TYPE` implementation should extract `table_column_def` into the
  shared type-literal production rather than duplicating it.
