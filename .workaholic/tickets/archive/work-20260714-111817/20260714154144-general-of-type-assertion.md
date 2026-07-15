---
created_at: 2026-07-14T15:41:44+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 4h
commit_hash: 6a5db6a
category: Added
depends_on: [20260709104254-blueprint-type-system-chapter.md]
mission: language-design-review-layering-principles-and-semantic-gaps
needs_design_brief: false
---

# General mid-pipe `of <type>` assertion — the use-site type contract as a first-class stage

## Overview

Blueprint §5.6 rules `of <name>` a **general, any-position, plan-time-checked type assertion** —
the `create table … of customer` vocabulary generalised, never a transform special case. Today `OF
<type>` is wired only at two *boundary* sites: the catalog write `CREATE TABLE <path> OF <name>`
(`grammar.rs:1557`) and the declared-view `CREATE VIEW … OF <name>` (`grammar.rs:2215`). The
**mid-pipe** form the mission's last-but-one acceptance box names —

```
… |> of customer |> …
```

— has **no parser production and no `PipeOp` variant**. `PipeOp` (`parser/src/ast.rs:177`) stops at
`Follow`; `pipe_op`'s dispatch `alt` (`grammar.rs:846`) has no `of_op`. The blueprint already lists
`of` in the stage-admission table (§5.3a criterion 2, "plan-time schema rewrite … `of`") and in the
§5.3 typing-rule prose, and the mission changelog records the general `of` assertion as *the item
the owner flagged to work through first*. This ticket makes the specced stage real.

This is a **closed-core change-control event** — adding the 20th `PipeOp` variant. Per §5.3a a new
stage must cite the admission criterion it satisfies: `of` satisfies **(2) plan-time schema rewrite
— it asserts/names the relation type**. It performs **no effect and no row transformation**: it is
schema-identity at runtime (`Relation<S> → Relation<S>`) and a *check* at plan time. That keeps it
outside the effect gate entirely (it is not in criterion 3), which is exactly why it is safe as a
pure assertion.

## Design (settled — blueprint §5.6, no new design)

`of <T>` asserts the incoming relation's type at that point and **never coerces** (`of` asserts,
`select`/`extend` transform). `<T>` is a type *reference by the §5.5 rule* — a **name** (bare or
qualified: `customer`, `chatwork/message`), never a `/type/…` path (path-form at a reference site is
the §5.7 category error and must be rejected). Two checks, mirroring §5.4's honest split:

- **Structural half — plan time.** Against the stage's computed schema `S` (known at every seam by
  §5.3), the asserted type's columns must match. A mismatch is a **plan-time structured error naming
  the differing columns** — never a runtime surprise, never a silent pass.
- **Refinement half — next boundary.** Where the named type carries a `WHERE` refinement, the
  structural half is plan-checked and the predicate half is membership at the next boundary rows
  exist — reusing `core::membership::check_membership` exactly as the declared-view `OF` boundary
  does (§5.4). No new predicate path.

**Inline structural form.** §5.6 also shows `|> transform triage of (priority text, reason text)` —
`of` over an **anonymous** type literal (the §5.2 column-list production), not only a named type.
The mid-pipe stage should accept both: `of <name>` and `of (<col> <type>, …)`. The named form
resolves through the existing `type_name` parser (`grammar.rs`, the same one `create type`/`OF` use);
the inline form reuses the `CREATE TABLE(cols)` column-literal parser.

**One genuine implementation choice — the transform-suffix surface** (resolve in the plan, see
below): whether `|> transform triage of (…)` is parsed as the transform stage *carrying* an `of`
annotation, or lowered to two ops (`transform triage` then `of (…)`). Recommendation: **implement
the standalone `PipeOp::Of` stage as the single mechanism**, and treat `transform … of …` as the
same `of` stage following the transform in the op vector (parser emits two ops, or a thin
parse-time desugar). Rationale: one typing rule, one checker arm, one execution no-op; the transform
stage stays exactly as shipped (§5.5, `transform <name>`), and `of` is genuinely general rather than
a transform-coupled suffix. The alternative (an `Option<OfRef>` field on `TransformRef`) special-
cases `of` back onto transform — the very coupling §5.6 rejects.

## Key Files (blast radius — every exhaustive `PipeOp` match must gain an arm)

- `crates/parser/src/ast.rs` — add `PipeOp::Of(OfRef)` (variant #20) with a doc-comment citing
  §5.6 + admission criterion (2), mirroring the `Transform`/`Follow` contextual-stage comments; add
  the `OfRef` struct (`{ target: OfTarget, span }`, where `OfTarget` is `Named(String)` |
  `Inline(Vec<ColumnDef>)` — reuse the existing column-literal AST).
- `crates/parser/src/grammar.rs` — add `of_op` to the `pipe_op` `alt` (`:846`); parse `word("of")`
  then either a `type_name` (reject a `/type/…` path at the reference site, as `create type` does)
  or a parenthesised column list. `of` stays a **contextual identifier — no new frozen keyword**
  (the 39-keyword freeze holds; `of` is already `word("OF")` in DDL).
- `crates/parser/src/tests.rs` — parse goldens: `of customer` accepted; `of (a text, b int)`
  accepted; `of /type/customer` **rejected** (category error, like `transform /path`).
- `crates/core/src/resolve.rs` — the plan-time `PipeOp` match: resolve the `of` target's name to its
  canonical `/type/…` catalog entry (same resolution the column-type / `OF` sites use) and run the
  **structural check** against the computed schema; emit the columns-differ structured error. This is
  the checker heart — the schema-at-seam must be threaded here (confirm where §5.3's per-stage schema
  is computed; `resolve.rs` already matches `PipeOp` exhaustively).
- `crates/core/src/eval.rs` — execution match: `of` is a **schema-identity no-op** on the row batch
  (rows pass through unchanged); wire the refinement-membership check at the boundary if the asserted
  type is refined and rows are present (reuse `check_membership`).
- `crates/exec/src/lib.rs`, `crates/exec/src/declared.rs`, `crates/exec/src/codec.rs` — add the
  pass-through arm wherever `PipeOp` is matched exhaustively.
- `crates/pushdown/src/lower.rs` — `of` is not pushable and is a local no-op that preserves the
  truthful residual; add the arm (assert schema-identity, keep the residual honest).
- `crates/http/src/rewrite.rs`, `crates/watchtower/src/bind.rs`, `crates/core/src/ddl/server.rs`
  (+ `server/spec.rs`) — add the exhaustive-match arm (server-binding lowering treats `of` as a
  transparent stage).
- `crates/plan/src/preview.rs` — verify PREVIEW renders an `of`-bearing pipeline honestly (the
  assertion adds no effect node; it should be invisible to the effect preview but must not drop the
  downstream stages).

## Considerations

- **Governance lock.** The `PipeOp` closed-core is asserted by a governance test (the
  exec-inventory / dep-direction family, `crates/cmd/tests/exec_inventory.rs`). Adding `Of` will trip
  it; update the locked inventory and add the §5.3a admission-criterion citation alongside the
  existing `Transform`/`Switch`/`Follow` entries — this is the deliberate, reviewed change-control
  step, recorded, not silent.
- **Never coerces (test it).** Add a plan-time test that `… |> of T` on a schema that *matches* T
  passes and leaves the schema byte-identical, and one where it *differs* fails naming the columns.
  A separate test that `of` does **not** rename/reorder/drop columns (it is not `select`).
- **Refinement at the boundary, not mid-pipe.** A refined `of T` mid-pipe checks structure at plan
  time; the predicate is checked at the next materialised-row boundary (write / declared-view
  delivery), consistent with §5.4 — do **not** invent a mid-pipe row-materialisation just to check
  the predicate early (that would violate "describe/preview touch nothing").
- **Reference form is a name (§5.5/§5.7).** `of /type/x` must be a structured rejection at parse or
  resolve, matching the `transform /path` and `create type /type/…` locks already shipped.
- **Docs + skills.** `of` becomes a documented pipe stage → `cargo run -p xtask -- gen-docs`
  regenerates `docs/language.md`; if any cookbook article gains an `of` recipe,
  `gen-skills` + the `cookbook_skills` ratchet must stay green. Blueprint §5.6's heading
  (`— blueprint, general rule`) and §5.3a's "all 19 PipeOp variants" count flip to *implemented* /
  *20 variants* — update the prose in the same PR so the blueprint stops describing this as unbuilt.
- **Plugin version.** `of` is a **new taught pipe stage** the qfs skills may reference → this is a
  skill-affecting surface addition. Bump all four plugin `version` fields (minor — new taught
  surface) and the qfs patch version, per CLAUDE.md.

## Implementation note (what shipped, and the one design correction)

The structural check landed in the **evaluator's schema fold** (`Evaluator::check_of_assertion` in
`core/src/eval.rs`), NOT in `resolve.rs` — `fold_op` is where the per-stage schema is computed, and
it describes the **addressed path** (`describe_schema(driver, "/sys/drivers")`), so it has the true
column schema. The named form resolves through a new `MountRegistry::declared_types()` registry
(the `transform_defs` twin), populated by `load_declared_type_defs()` in the binary (`shell.rs`) from
the `/sys/drivers` `kind='type'` rows via the existing `resolve_type_def`.

**The correction (found by driving the real binary):** a pure read reaches ONLY the pushdown
lowering, whose leaf `schema_of` describes the driver **ROOT**, not the addressed sub-path — so a
lowering-side structural check saw an empty/root schema and falsely reported every asserted column as
missing. The fix mirrors `transform`'s read reclassification: a statement carrying an `of` stage is
routed through the evaluator (`build_plan`, exec `contains_of`), where the fold's addressed-path
schema drives the correct check; the pushdown `of` stays the schema-identity no-op it is. Verified
end-to-end against `/sys/drivers`: wrong column-set → `of_assertion_failed` (missing/unexpected),
wrong type → `of_assertion_failed` (mismatched), unknown name → `of_type_unresolved`, exact match →
rows returned, plain reads unaffected.

## Quality Gate

- `cargo test --workspace` green; new parse goldens + checker tests (match/mismatch/inline/refined/
  path-form-rejection) added.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `cargo run -p xtask -- gen-docs --check`, `gen-skills --check` all clean.
- The `PipeOp` governance/inventory test updated and green with the §5.3a citation.
- Blueprint §5.6 / §5.3a prose updated to *implemented*; mission acceptance box for the general
  mid-pipe `of <type>` assertion tickable.
