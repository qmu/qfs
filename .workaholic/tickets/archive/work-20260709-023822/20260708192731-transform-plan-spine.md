---
created_at: 2026-07-08T19:27:31+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash: 84c1d01
category: Added
depends_on: [20260708192730-transform-definition-ddl-storage.md]
mission:
---

# Add the transform plan spine: logical/physical lowering and forced-local planning

## Overview

Second of four dependency-ordered transform tickets (supersedes the deleted mega-ticket
`20260708002200`; design: archived brief `20260708002100` + blueprint §15, Decision W). With the
definition DDL landed (`depends_on`), this ticket builds the **pure plan spine**: the
schema-transforming `LogicalPlan::Transform` / `CombineOp::Transform` nodes, the lowering arm that
replaces today's `transform_not_executable` refusal, planner enforcement that a transform is
**never pushed to a source**, and the schema fold that exposes the definition's OUTPUT schema to
downstream stages. No execution — the exec boundary keeps refusing truthfully until the next
ticket lands the applier and routing.

**Discovery state (HEAD 24c2269):** `LogicalPlan` has 12 variants
(`crates/pushdown/src/logical.rs:145-249`) and `CombineOp` has 11
(`crates/pushdown/src/physical.rs:114-144`) — neither has `Transform`. The refusals to replace:
`lower.rs:312-314` (`LowerError::TransformNotExecutable`) and `core/src/eval.rs:675-677` (schema
fold).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the spine lives in the existing
  `pushdown` (logical/physical/planner) and `core` (eval/resolve) crates; no new crate.
- `workaholic:implementation` / `policies/coding-standards.md` — exhaustive matches on the grown
  enums; no catch-all arms that would silently pass a future variant.
- `workaholic:implementation` / `policies/type-driven-design.md` — the node carries the resolved
  output `Schema` + `Provenance`; downstream stages type-check against it at plan time.
- `workaholic:implementation` / `policies/functional-programming.md` — the planner and schema fold
  stay pure (build data, no I/O); the impure model call belongs to the next ticket's applier.
- `workaholic:implementation` / `policies/test.md` — hermetic plan-shape tests; no provider, no
  network.

## Key Files

Verified anchors at HEAD `24c2269` (2026-07-08):

- `packages/qfs/crates/pushdown/src/logical.rs:145-249` — `LogicalPlan` (12 variants): add
  `Transform { input, output_schema, … }`; extend `single_source()` (`:278-297`).
- `packages/qfs/crates/pushdown/src/physical.rs:114-144` — `CombineOp`: add `Transform`; extend
  `label()` (`:146+`).
- `packages/qfs/crates/pushdown/src/lower.rs:312-314` — replace `TransformNotExecutable` with the
  lowering arm (schema-transforming — NOT a pass-through like Decode/Encode).
- `packages/qfs/crates/pushdown/src/planner.rs:203-286` — `local_pinned`/`force_local` + the
  per-stage walk: a transform stage pins itself and everything after it local (never pushed to a
  source scan).
- `packages/qfs/crates/core/src/eval.rs:675-677` — replace the schema-fold refusal: a mid-pipe
  transform folds to the definition's OUTPUT schema so downstream `where`/`order by`/`select`
  type-check against it.
- `packages/qfs/crates/core/src/resolve.rs:392` — the resolve-walk arm (currently pass-through);
  keep coherent with definition resolution from the DDL ticket.
- `packages/qfs/crates/parser/src/ast.rs:722-731` — `TransformRef { name, span }` — the pipe-side
  reference the lowering resolves through the definition store.

## Related History

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) — settled: local/non-pushable, schema-transforming, three modes derived from INPUT shape.
- Commit `be4df97` — the grammar seam this spine executes; its tests prove `transform` composes with
  keyword stages and stays a plain identifier elsewhere.
- `docs/blueprint.md:563` — §15: plan shape and mode semantics.

## Implementation Steps

1. Add `LogicalPlan::Transform` carrying the resolved definition handle, its OUTPUT `Schema`, and
   `Provenance` for converted columns; extend `single_source()` and any exhaustive walks.
2. Add `CombineOp::Transform` + `label()`; thread through physical planning.
3. Replace the `lower.rs:312` refusal with the lowering arm: resolve `TransformRef` → definition
   (from the DDL ticket's resolution API), derive the mode (the definition's total function), and
   emit the schema-transforming node.
4. Planner: a transform stage is forced local (`force_local`) — it never reaches a source scan; the
   existing "everything after a local stage stays local" property covers the tail.
5. Schema fold (`core/eval.rs:675`): a transform folds to OUTPUT schema; declared input columns
   match by name and surplus incoming columns are ignored; a missing declared input column is a
   plan-time structured error.
6. Keep the exec boundary refusing execution truthfully (a structured "not yet executable" at the
   exec layer only — the plan itself is now well-formed) until the execution ticket lands; do not
   leave a silent passthrough.
7. Update the closed-core variant-governance/lock tests deliberately in the same reviewed change
   (the pipe lock stays 19; plan-side locks grow by exactly the reviewed variants).

## Quality Gate

Distributed from the parent mega-ticket's gate (owner-approved 2026-07-08); plus the common gate.

**Acceptance criteria:**

- Lowering: `/src |> transform <def>` lowers to `LogicalPlan::Transform` with the definition's
  OUTPUT `Schema` + `Provenance` (unit-asserted).
- Planner: the transform node is never pushed to a source scan (test proves forced-local, including
  a source that could otherwise take the whole pipeline).
- Schema fold: a mid-pipe `transform` followed by `where`/`order by`/`select` exposes the OUTPUT
  schema to those stages (test); input columns match by name and surplus incoming columns are
  ignored (test); empty/missing declared input column fails structurally at plan time.
- All three modes produce the correct plan shape from the derived mode (row-wise, relation-wise,
  extraction) — plan-shape tests, no execution.
- `keyword_count_is_frozen` still 39; pipe-variant lock still 19; plan-side governance locks updated
  by exactly the reviewed new variants.
- Exec still refuses execution with a truthful structured error (no silent no-op rows).

**Verification method:**

- Hermetic: `cargo test -p qfs-pushdown -p qfs-core -p qfs-parser -p qfs-lang` (workspace when disk
  allows); `gen-docs --check`; `clippy --workspace --all-targets -D warnings`; `fmt --all --check`.

**Gate:** all green, purely hermetic; no provider code exists yet in this ticket.

## Considerations

- Depends on `20260708192730` for definition resolution + the mode total function — do not
  re-derive mode logic here; consume it.
- The execution/routing ticket (`20260708192732`) replaces the temporary exec-layer refusal; keep
  that refusal in one obvious place so it deletes cleanly.
- Forced-local composes with the pushdown honesty property (t20): the pushed sub-query may
  over-return; residuals still re-check locally.
- Experimental / no backward compat: grow the enums definitively; no feature flags.

## Delivered (2026-07-09)

Implemented per the design below, with ONE refinement: instead of threading a resolver param
through `plan_pipeline`/`Evaluator`/exec (~8 sites), the resolved definitions ride a new
**`MountRegistry.transform_defs`** field (`set_transform_defs`/`transform_defs()`) — both the
lowering (`plan.rs` builds a `transform_of` closure over it) and the evaluator fold
(`self.mounts.transform_defs()`) already hold `mounts`, so no signature churn. The binary
(`shell.rs::register_cloud_and_sys_mounts`) installs `crate::transform::load_transform_defs()` (DB
scan → `TransformDef::from_stored` → `ResolvedTransform::new`) on the engine before planning.
Landed: `qfs-types` `ResolvedTransform`/`TransformDefs`; `LogicalPlan::Transform`
(`single_source`→None) + `CombineOp::Transform` + `explain`; `lower.rs` lowering arm (Provenance-
tagged OUTPUT); planner `walk_chain` force-local + `federate`; `eval.rs` `PlanSource::Transform`
folding to OUTPUT with by-name input matching (`EvalError::TransformInputMissing`); the SINGLE
exec-boundary refusal `EngineError::TransformNotExecutable` in `engine/combine.rs` (T3 deletes it).
Tests: pushdown lowering + forced-local + unresolved; core fold/surplus/missing/unresolved; engine
refusal. All T1 gates green (per-crate tests, clippy `-D warnings`, fmt, gen-docs/gen-skills/
check-migrations, dep_direction). No plan-variant lock test existed to update; pipe/keyword locks
(19/39) untouched.

## Implementation Design (mapped 2026-07-09, T1 landed at `5c22fe7`; execution NOT started)

T1 shipped the definition side. A pre-implementation walk of T2 found it is a **T1-sized cross-crate
change** touching **two planning paths** (both refuse execution today, both must change). Full plan:

### 1. The definition resolver (the crux — how DB-resident definitions reach the pure planner)
Per-definition INPUT/OUTPUT schemas live in the System DB (`sys_transforms`), which the pure
`qfs-pushdown`/`qfs-core` cannot read. So thread a resolved-definition map from the binary:
- **`qfs-types`**: add `ResolvedTransform { input: Schema, output: Schema, mode: TransformMode }`
  (+ `ResolvedTransform::new(input, output)` deriving the mode) and
  `pub type TransformDefs = BTreeMap<String, ResolvedTransform>`. Both `pushdown` and `core` reach
  the leaf `qfs-types`. (This exact addition was drafted and compiled, then reverted to keep the
  tree clean at `5c22fe7` — re-add it first.)
- Thread `&TransformDefs` (empty = no definitions wired ⇒ a transform stage lowers/folds to a
  structured "unresolved" error) through `lower_query` (`pushdown/src/lower.rs:142`, alongside
  `source_of`/`schema_of`) and add an `Option<&'r TransformDefs>` field to `Evaluator`
  (`core/src/eval.rs:295`, via `new`/`with_stdlib` + a `with_transforms` builder).
- Callers to update (~8): `core/src/plan.rs:96` `plan_pipeline` (add a param — it currently has only
  `mounts`; the resolver must come from the caller), `core/src/lib.rs`, `exec/src/exec.rs`. The
  **binary** builds the `TransformDefs` from the DB (`TransformDbBackend::scan` →
  `qfs_core::ddl::transform::TransformDef::from_stored` → `ResolvedTransform::new`) before planning.

### 2. Pushdown path (`plan_pipeline` → `lower` → `partition_by_source`)
- `logical.rs:145` add `LogicalPlan::Transform { input: Box<LogicalPlan>, name: Name,
  output_schema: Schema, mode: TransformMode }`. In `single_source()` (`:278-297`) → **`None`** (a
  transform-bearing subtree is NEVER a native-pushdown/join candidate — forced local).
- `physical.rs:114` add `CombineOp::Transform { name, input_schema, output_schema, mode }` +
  `label()` (`:146`) → `"Transform"`.
- `lower.rs:308-315`: replace `LowerError::TransformNotExecutable` with the lowering arm — look up
  `transform_of(name)`; emit `LogicalPlan::Transform` carrying the resolved OUTPUT schema +
  `Provenance` (converted columns tagged to the transform). An unresolved name is a structured
  lower error. Keep the `TransformNotExecutable` variant only if still needed; otherwise remove it.
- `planner.rs`: `single_source_chain(Transform{input})` → `single_source_chain(input)` (delegate);
  `scan_path(Transform{input})` → `scan_path(input)`; **`walk_chain`** (`:230`) →
  `walk_chain(input, acc)` then `acc.force_local(CombineOp::Transform{..})`, return
  `output_schema.clone()` (the existing `local_pinned()` property keeps every later stage local —
  that IS the forced-local proof); **`federate`** (`:331`) → `combine1(CombineOp::Transform{..},
  partition_by_source(input, reg)?)` for a cross-source input.

### 3. Core evaluator path (`eval.rs::fold_query` → `PlanSource`)
- `eval.rs:672-677`: replace the `EvalError::TransformNotExecutable` schema-fold refusal — resolve
  via `self.transforms`, fold to the definition's **OUTPUT** schema so downstream `where`/`order
  by`/`select` type-check against it. Declared input columns match by name; **surplus incoming
  columns are ignored; a missing declared input column is a plan-time structured error**. Likely a
  new `PlanSource::Transform { input, schema }` variant (or reuse a shape node with a replaced
  schema) — check `PlanSource`'s exhaustive matches.
- `resolve.rs:392`: the `PipeOp::Transform(_)` arm is a pass-through `Ok(())` (name/capability gate);
  keep it coherent — resolution proper happens at lowering/fold, not here.

### 4. Execution stays refused in ONE obvious place (deleted by T3)
The plan is now well-formed but must NOT run a model. Keep a single truthful refusal at the
**execution boundary**, not in planning: the **`qfs-engine` `CombineEngine`** arm for
`CombineOp::Transform` (find its exhaustive `CombineOp` match) returns a structured
"transform not yet executable" error; and/or the interpreter's `PlanSource::Transform` executor.
Do NOT leave a silent passthrough (no no-op rows). T3 replaces exactly this with the applier.

### 5. Closed-core governance locks
- `keyword_count_is_frozen` (39) and the **pipe-variant lock (19)** are UNAFFECTED — no new
  `PipeOp` (parser/src/tests.rs). Do not touch them.
- The **plan-side** variant-governance/lock tests grow by exactly the reviewed variants: find the
  `LogicalPlan`/`CombineOp` (and any `PlanSource`) count/lock tests in `qfs-pushdown`/`qfs-engine`/
  `qfs-core` and update them deliberately in the same reviewed change.

### Verified anchors (HEAD `5c22fe7`)
`pushdown/src/logical.rs:145,278`; `pushdown/src/physical.rs:114,146`; `pushdown/src/lower.rs:142,308`;
`pushdown/src/planner.rs:108,123,162,230,331`; `core/src/plan.rs:96`; `core/src/eval.rs:295,528,672`;
`core/src/resolve.rs:392`; `types/src/transform.rs` (add `ResolvedTransform`/`TransformDefs`).
Build bottom-up (types → pushdown → core → exec → binary), compiling each crate; run the full T1
gate set (per-crate tests + `clippy --workspace --all-targets -D warnings` + `fmt --all --check` +
gen-docs/gen-skills/check-migrations + `dep_direction`).
