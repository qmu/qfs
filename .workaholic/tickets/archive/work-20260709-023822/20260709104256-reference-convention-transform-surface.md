---
created_at: 2026-07-09T10:42:56+09:00
author: a@qmu.jp
type: refactoring
layer: [Domain]
effort: 4h
commit_hash: 357ebf2
category: Changed
depends_on: [20260709104254-blueprint-type-system-chapter.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Reference-convention ruling + transform stage surface — paths = data, names = definitions

## Overview

Rule the reference-convention inconsistency the owner raised, and apply it to the concrete
surface. **Time-critical: must land before T2 wires `PipeOp::Transform` execution**
(`20260708192732-transform-execution-routing.md`), because after execution wiring any
reference-syntax change spreads into the exec layer, preview rendering, and audit records.

**The ruling (from the mission, grounded in the typed-path space): a stage verb's operand has a
category** — it is either **inflowing data** (a path, because a path is a typed data-location:
`join /sql/erp/products`, `union /path`) or a **behavior-selector parameter** (a bare name/token
that selects a registered behavior: `decode json`, `call mail.send`, `transform triage`). A path in
selector position is a **category error** — data-location syntax naming a function. So paths = data,
names = definitions: definitions stay stored and inspectable at catalog paths (`ls /transform`,
`describe /transform/triage` — the shell face), and are invoked by name (the stage face); **the pipe
never applies a path.** transform is in the `decode`/`call` selector family, so `|> transform triage`.
(Nuance for the type chapter: `transform`'s selector resolves against a *mutable* registry like
`call`'s name, not a *frozen* vocabulary like `decode json` — so if `/transform` ever nests, the
selector may need `billing/triage`-style qualification; flat `/transform/<name>` needs only a bare
name today.)

Consequences applied here:

- The transform stage reads `|> transform <name>` (verb + object, no `transform /transform/…`
  stutter). The shipped T1 parser **already** takes a bare ident (`crates/parser/src/grammar.rs`,
  `transform_op` → `TransformRef.name`), so this is mostly a **blueprint §15 correction** to match
  the code, plus a confirming parser test — not a grammar change.
- The `CREATE VIEW` path-vs-bare pun (`crates/parser/src/tests.rs:877` — declared-view vs
  server-binding dispatched by reference style alone) becomes **principled**: path = readable data
  surface, bare name = server binding. Document the rule; do not change the dispatch behaviour
  unless the ruling demands it.
- **Type references are name-ified** (a type is a definition, so referencing it by path is the same
  category error as transform's): `of /type/customer` → `of customer`; a column type
  `email /type/email` → `email: email`; `create type /type/customer` → `create type customer` (the
  `TYPE` noun implies the `/type` mount, matching shipped `CREATE TRANSFORM <name>`). `/type` stays
  the catalog/shell face (`ls /type`, `describe /type/customer`). Base and refined type names unify
  into **one namespace** — fixing the current split where `id int` is bare but `email /type/email`
  is a path in the same column-type position (`create_type_stmt`/`table_column_def`, `grammar.rs`).
  The type chapter owns the canonical statement; this ticket makes the grammar/parser change and
  corrects the blueprint examples. The transform namespacing nuance (mutable-registry resolution,
  qualified names if `/type` or `/transform` nests) applies identically to types.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — parser/grammar edits stay in
  `qfs-parser`; catalog semantics stay where the registry lives.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to parser test + any
  grammar doc-comment edits.
- `workaholic:implementation` / `policies/type-driven-design.md` — the paths/names split is a
  type-level distinction (data vs definition); the ruling must be expressed so the parser makes
  the wrong shape unrepresentable, not merely rejected late.
- `workaholic:implementation` / `policies/objective-documentation.md` — the §15 correction must
  make spec and code agree (currently drifted).

## Key Files

- `docs/blueprint.md` §15 - the `/transform/…` stage examples corrected to `transform <name>`;
  the paths = data / names = definitions rule stated (cross-referencing the type chapter).
- `packages/qfs/crates/parser/src/grammar.rs` - `transform_op` (already name-taking); add/confirm
  a test that a `transform /path` form is rejected, locking the ruling.
- `packages/qfs/crates/parser/src/tests.rs` - `CREATE VIEW` dispatch tests (line ~877); document
  the path-vs-name rule at that dispatch; add type-reference name-form tests.
- `packages/qfs/crates/parser/src/ast.rs` - `TransformRef` doc-comment reflects the ruling.
- `packages/qfs/crates/parser/src/grammar.rs` - `create_type_stmt` (~1792, `/type/…` path → bare
  name; `TYPE` noun implies the mount), `table_column_def` (a column type accepts a bare refined
  type name alongside base `ColumnType` names — one namespace), and the `OF` clause
  (`create_table_stmt` / `create_declared_view_stmt`, `of /type/x` → `of x`).

## Related History

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) - the §15 design brief whose path examples this corrects
- [20260626103000-t70-operator-equals-binds-eqeq-compares.md](.workaholic/tickets/archive/work-20260628-000332/20260626103000-t70-operator-equals-binds-eqeq-compares.md) - precedent for a deliberate surface-token ruling recorded in the grammar

## Implementation Steps

1. Record the paths = data / names = definitions ruling in the blueprint (a short reference-model
   section, cross-referencing the type chapter's catalog-vs-reference split).
2. Correct blueprint §15's `CREATE TRANSFORM /transform/triage` and `|> transform /transform/…`
   examples to the name form (`CREATE TRANSFORM triage`, `|> transform triage`), matching shipped
   T1.
3. Add/confirm a parser test asserting `|> transform <name>` parses and `|> transform /path` does
   not — the lock for the ruling.
4. Document the `CREATE VIEW` path-vs-bare dispatch as the principled rule at
   `crates/parser/src/tests.rs` and the relevant grammar doc-comment.
5. **Name-ify type references** (the real grammar change): `create type <name>` (noun implies the
   `/type` mount), a bare refined-type name in `table_column_def` type position (one namespace with
   base types), and `of <name>` in `CREATE TABLE`/declared-view `OF`. Resolve names against the
   `/type` catalog. Correct blueprint §5/§105 type examples to the name form. Add parser tests.
6. Full anti-drift suite; tick the mission's reference-convention acceptance box.

## Quality Gate

**Acceptance criteria:**
- Blueprint states the paths = data / names = definitions rule and §15 examples use the name form.
- A parser test locks `transform <name>` accepted / `transform /path` rejected.
- `create type <name>`, a bare refined-type name in a column-type position, and `of <name>` parse
  and resolve against the `/type` catalog; the old `/type/…` reference forms are corrected in the
  blueprint; a parser test covers the name forms (base + refined types in one namespace).
- The `CREATE VIEW` dispatch is documented as principled (path = data surface, name = binding).
- Spec (§5/§15/§105) and the parser no longer drift.

**Verification method:**
- `cd packages/qfs && cargo test --workspace` (the new/confirmed parser test passes).
- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` green.
- Owner confirms the ruling and that it is settled **before** the T2 execution ticket is driven.

**Gate:** anti-drift suite green + the before-T2 sequencing confirmed with the owner; mission box updated.

## Considerations

- **Sequencing is the main risk**: this ticket must be driven before
  `20260708192732-transform-execution-routing.md` (T2). Flag it in the drive queue ordering.
- Depends on the type chapter for the catalog-vs-reference split's canonical statement; this
  ticket applies it, it does not re-decide it.
- Do not redesign `CREATE VIEW` dispatch mechanics here unless the ruling forces it — the goal is
  to make the existing behaviour principled and documented, not to churn the parser.
