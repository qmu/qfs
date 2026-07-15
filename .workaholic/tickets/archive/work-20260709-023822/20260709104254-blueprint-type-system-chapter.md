---
created_at: 2026-07-09T10:42:54+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort: 4h
commit_hash: cee3548
category: Added
depends_on:
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Blueprint type-system chapter — one vocabulary, first-class relation types, catalog-vs-reference

## Overview

The **root ticket** of the language-design-review mission: every other gap in that mission is a
symptom of a retrofitted type system, so this ticket writes the missing foundation as a blueprint
chapter that the sibling tickets derive from. It is **mostly a design/prose deliverable** (blueprint
+ its cross-references) **plus one concrete implementation slice** — the refinement-predicate
`WHERE` clause that is specced but unbuilt (step 6). The engine already carries `qfs_types::{Schema,
ColumnType}`; what is missing is the written-down theory that makes the three type spellings one,
positions types as definitions, states a typing rule per stage, and — the owner's framing — defines
the type system as the **typed-path space**.

**The typed-path space (the owner's "shell + querying integration").** A qfs path is a **typed
location**, not a string address: one path type governs three operations — navigation (`cd`/`ls`,
the shell face), query (reading the path yields rows in its type, the querying face), and
write-membership (only type-conforming data may be placed there — a Gmail path admits only
Gmail-shaped rows). `Schema` **is** the type of a path; the shell and query faces are two operations
over one typed namespace. This is the chapter's spine, and it decides the **stage-operand category
rule**: a stage verb's operand is either inflowing data (a path — `join /sql/...`) or a
behavior-selector parameter (a bare token — `decode json`, `call mail.send`, `transform triage`);
a path in selector position is a category error. Definitions live at typed catalog paths (shell
face) and are invoked by name (stage face).

Evidence the chapter must reconcile (verified against HEAD, 2026-07-09):

- One vocabulary spelled three ways: column types `text`/`int`/`bytes` (`ColumnType::parse`,
  `crates/types/src/schema.rs:127`), lambda annotations `string`/`i64`/`Row` (`TypeAnn`,
  parse-and-retain, `crates/parser/src/ast.rs:561`, unenforced until t75), and the transform DDL
  holding raw type strings rehydrated later by `ColumnType::parse` (`crates/parser/src/grammar.rs`
  around the `create_transform_stmt` clauses).
- `ColumnType::Unknown` is a live vocabulary word and `reduce` returns it — a "not yet known"
  token baked into the type language.
- The blueprint conceives an entity type as "a named, path-addressed, intensional relation"
  (`docs/blueprint.md:100`), colliding with the reference principle this mission adopts
  (paths = data, names = definitions).
- Spec/implementation drift: blueprint §15 writes `CREATE TRANSFORM /transform/triage`, but the
  shipped T1 parser takes a **bare ident** name (`crates/parser/src/grammar.rs`,
  `create_transform_stmt`).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the chapter dictates where
  type vocabulary/grammar lives (leaf `qfs-types`); keep the acyclic spine intact.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to any doc-comment
  edits that cite the chapter.
- `workaholic:implementation` / `policies/type-driven-design.md` — the core lens: this chapter
  IS the project's type-driven-design statement for the language; every typing rule must make a
  domain gap machine-checkable early.
- `workaholic:implementation` / `policies/functional-programming.md` — relation types as the
  domain of a typed composition (pipeline = typed function composition) is the FP framing the
  chapter records.
- `workaholic:implementation` / `policies/objective-documentation.md` — the chapter's claims
  (per-stage typing rules, catalog-vs-reference split) must be stated as verifiable rules, not
  aspirations.

## Key Files

- `docs/blueprint.md` - the new chapter lands here (near §5 type literal / §15 transform); §100
  entity-type definition and §15 examples are revised to match.
- `packages/qfs/crates/types/src/schema.rs` - `ColumnType`/`Schema`, the single canonical
  vocabulary the chapter blesses.
- `packages/qfs/crates/parser/src/grammar.rs` - `create_type_stmt` (~line 1791) gains the optional
  `WHERE <predicate>` (reusing the `predicate` parser); `type_columns_json` stores it.
- `packages/qfs/crates/core/src/typeck.rs` - declare-time well-formedness check of the refinement
  predicate against the declared columns (the checker already types `WHERE` predicates).
- `packages/qfs/crates/core/src/eval.rs` - eval-time membership: run the stored predicate per
  candidate row at the existing structural-membership point (`eval` ~line 429).
- `packages/qfs/crates/parser/src/ast.rs` - `TypeAnn` (the `string`/`i64` spelling the chapter
  retires); doc-comment updated to cite the chapter and the sibling stdlib ticket.
- `docs/roadmap.md` - t75 is repositioned as the chapter's enforcement, not a bolt-on.

## Related History

The type surface grew slice by slice; this chapter unifies what those slices each introduced.

- [20260622214650-t05-type-schema-model.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t05-type-schema-model.md) - introduced `ColumnType`/`Schema` (the canonical vocabulary)
- [20260704124825-design-entity-type-system.md](.workaholic/tickets/archive/work-20260703-194046/20260704124825-design-entity-type-system.md) - the path-addressed entity-type framing this chapter reconciles
- [20260627120200-t75-static-primitive-type-system.md](.workaholic/tickets/archive/work-20260628-000332/20260627120200-t75-static-primitive-type-system.md) - the deferred static checker, repositioned as this chapter's enforcement
- [20260626101900-t61-lambdas-higher-order-fns.md](.workaholic/tickets/archive/work-20260628-000332/20260626101900-t61-lambdas-higher-order-fns.md) - added `TypeAnn` and the `Unknown`-returning `reduce`

## Implementation Steps

1. Write the blueprint type-system chapter with these rulings:
   - **The typed-path space** — a path's type governs navigation (`cd`/`ls`), query (path read →
     typed rows), and write-membership as one; `Schema` is the type of a path; the shell face and
     query face are operations over one typed namespace. This is the chapter's spine.
   - **The stage-operand category rule** — a stage verb's operand is inflowing data (a path) or a
     behavior-selector parameter (a bare name); a path in selector position is a category error.
     (The reference-convention ticket applies this; state the rule here.)
   - **One vocabulary, one grammar** used everywhere a type appears (DDL `INPUT/OUTPUT`, lambda
     annotations, DESCRIBE, driver declarations). Retire the `string`/`i64`/`Row` annotation
     spellings in favour of the `ColumnType` grammar; the sibling stdlib ticket enforces it.
   - **Relation types are first-class**, so a pipeline is a typed composition and **every stage
     has a typing rule** `Relation<S> → Relation<S'>` — state this as the formal content the
     stage-admission-test ticket references.
   - **Named types are definitions** carrying **refinement predicates** (`CREATE TYPE … WHERE
     <pred>`), name-referenced; `/type` and `/transform` are catalog **inspection** surfaces, not
     applied paths. State the refinement model: row-local **pure** predicates, checked as
     membership at the write/`OF` boundary, statically well-formedness-checked at declare time —
     explicitly **not** proof-carrying/solver-discharged refinement (out of scope; contract-checked
     like a CHECK constraint, not statically proven over arbitrary pipelines).
   - **`of <type>` use-site assertion** as a general, any-stage, plan-time-checked rule
     (generalising `create table … of /type/customer`), explicitly not a transform special case.
   - **Transform-surface considered-alternatives** recorded verbatim from the mission (bare-name
     endgame deferred; expression-layer call rejected permanently; inline anonymous form as
     future sugar; use-site `of` generalised).
2. Revise `docs/blueprint.md:100` (entity type as path-addressed relation) and §15's
   `/transform/…` examples so the prose is consistent with the name-reference ruling.
3. Update the `TypeAnn` doc-comment (`ast.rs`) and t75's roadmap entry to cite the chapter.
4. **Implement the refinement `WHERE` slice** (the one code change in this ticket — the specced
   §105 `CREATE TYPE … WHERE <pred>` is unbuilt today):
   - Parse an optional `WHERE <predicate>` in `create_type_stmt` (`grammar.rs`), reusing the
     existing `predicate` parser; store it in the `/sys/drivers` type row's `body` JSON alongside
     the columns.
   - At **declare time**, run t75's `typeck` over the predicate against the declared columns so a
     malformed refinement (unknown column, non-boolean predicate) fails at CREATE, not at write.
   - At **write/`OF` membership**, evaluate the stored predicate per candidate row with the pure
     predicate evaluator (`eval.rs`), at the same eval-time point that already checks structural
     schema; a failing row is a structured error naming the column/predicate (no I/O, pure).
   - Restrict refinements to row-local pure expressions (`LIKE`, comparisons, stdlib scalars,
     arithmetic if adopted); reject a predicate that references another path/relation or an
     aggregate (not row-local, not decidable-pure) with a structured declare-time error.
5. Run the anti-drift suite; add a cookbook recipe exercising a refined type end-to-end
   (declare → conforming write passes → violating write refused) as the verified-true ratchet.
6. Tick the mission's type-system-chapter acceptance box with this ticket's filename.

## Quality Gate

**Acceptance criteria:**
- The blueprint contains a type-system chapter covering all rulings in step 1 (typed-path space,
  stage-operand category, one vocabulary, first-class relation types, refinement model, `of`
  assertion, considered-alternatives), each stated as a verifiable rule.
- `docs/blueprint.md:100` and §15 no longer contradict the name-reference ruling.
- **`CREATE TYPE … WHERE <pred>` works end-to-end**: a conforming `VALUES`/`OF` write passes, a
  violating one is refused with a structured error naming the column/predicate; a malformed
  refinement (unknown column / non-boolean / cross-relation reference) fails at **declare** time.
- No dangling reference: t75 roadmap entry and the `TypeAnn` doc-comment point at the chapter.

**Verification method:**
- `cd packages/qfs && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- New tests: refinement declare-time well-formedness (accept/reject), and eval-time membership
  (conforming row accepted, violating row a structured error) — hermetic, no I/O.
- `cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` both green;
  the refined-type cookbook recipe passes `crates/test/tests/cookbook_skills.rs`.
- Owner reads the chapter end-to-end and confirms it is the foundation the sibling tickets cite.

**Gate:** full suite + refinement tests + anti-drift + cookbook ratchet green + owner read-through;
mission checkbox updated.

## Considerations

- This is the foundation ticket — the sibling tickets (`depends_on` this one) must not begin
  their code changes until the chapter's rulings are settled, or they will encode a moving target.
- Keep the vocabulary in the leaf `qfs-types` crate; the chapter must not push type logic up into
  the parser or core in a way that breaks the acyclic spine (`directory-structure`).
- No `Experimental` hedging about migration — pre-release, hard breaks in type spelling are
  correct (mission out-of-scope note; user memory: experimental-no-backward-compat).
