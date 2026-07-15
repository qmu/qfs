---
created_at: 2026-07-13T19:50:08+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
needs_design_brief: false  # satisfied 2026-07-14 — see "## Design brief" below; ready to implement
---

# Give the effect node a predicate/selector channel so a per-row folder rename is representable

> **Deferred by owner decision (2026-07-13).** This is a **design-layer** change to the shared
> `EffectNode` representation in `qfs-core`, not a contained driver fix. It carries a real semantics
> decision and touches the plan type every driver's applier decodes, so it is routed through a
> **design brief first** (write the brief — Fable-grade design judgment — then implement), rather
> than an ad-hoc `/drive`. Left in the queue as the capture. Closes concern
> `37-new-drive-folder-rename-predicate-channel.md` when done.

## Overview

`UPDATE /drive/my/<folder> SET name='X' WHERE name='Y'` cannot rename the matching child today: the
v0.0.60 fix (`ec67ae6`) makes a name-path folder UPDATE **refuse loudly** (safe) instead of
silently mutating the container. The root cause is representational — `core/src/eval.rs`'s
`setwhere_row_batch` flattens `SET` and `WHERE` into a single `RowBatch` and **de-dups by column
name** (`if !cols.iter().any(|c| c.name == name)`), so a `WHERE` key that shares the `SET` column is
dropped. `EffectNode` carries a single `args: RowBatch` with no separate selector channel, so the
driver's `decode_move` cannot tell the selector (`'Y'`) from the new value (`'X'`).

To rename the matching child (the richer, non-refusing behaviour) the effect representation needs a
**predicate/selector channel distinct from the SET row payload**, which then flows to
`gdrive::effect::decode_move` so it can resolve the child-by-selector under the folder and rename it.

## Why this needs a design brief (not a drive)

- **Shared representation**: the channel must live on `EffectNode` in `qfs-core` — the plan type
  EVERY driver's applier decodes (gmail, sql, slack, cf, git, gdrive, fs, local…). The decision
  affects all of them, even if only gdrive consumes it first.
- **Semantics to decide**:
  - Multi-match: when the `WHERE` selector matches ≥2 children, refuse as ambiguous (mirroring
    `resolve_node`'s existing `AmbiguousTarget`) vs. rename all vs. require a narrower key. Proposed
    default: **refuse ambiguous**, matching the existing resolve semantics.
  - How the selector channel composes with the existing **file rename by name path** (which already
    works via a different route) and with **folder moves** (`add/remove parents`).
  - Whether the selector is a **gdrive-specific** decode concern or a **general `EffectNode`
    feature** (e.g. sql `UPDATE … WHERE key = <same column being SET>` has the same latent shape —
    audit whether other drivers silently rely on the current de-dup).
- **Blast radius**: `setwhere_row_batch`, the `EffectBody::SetWhere` lowering, `EffectNode`
  construction, plan preview/commit, and any plan/effect golden snapshots. (Experimental repo — hard
  breaks are fine; the point is the *design*, not backward compat.)

## Key Files (starting points for the brief)

- `packages/qfs/crates/core/src/eval.rs` — `setwhere_row_batch` (the de-dup), the
  `EffectBody::SetWhere` lowering at ~line 952.
- `packages/qfs/crates/core/` — the `EffectNode` struct (add the selector channel).
- `packages/qfs/crates/driver-gdrive/src/effect.rs` — `decode_move` (the refusal to replace with a
  child-by-selector resolve + rename).
- The living design blueprint — record the effect-representation decision there (per the
  blueprint-over-ADR convention).

## Considerations

- The current v0.0.60 behaviour is a **safe, loud refusal** with a documented workaround
  (`/drive/id:<id>` renames a folder; file renames by path work), so there is **no correctness bug
  outstanding** — this is a missing feature at severity **low**. It is safe to sit in the queue
  until the design brief is written.
- Prefer the smallest general change that also resolves the latent sql same-column shape, if the
  brief finds the two are the same decision.

## Policies

Derived from `layer: [Domain]` → `workaholic:implementation` (effect representation, plan/runtime
agreement) plus `workaholic:design` (the shared-representation design decision).

## Design brief (2026-07-14) — the effect selector channel

> **Provenance.** This is the Fable-grade design deliverable the ticket asked for. Fable's quota was
> exhausted at authoring time, so it was written on Opus against fully-gathered ground truth
> (file:line-verified below); it is decisive enough to write the implementation ticket from
> directly. **Reviewed by Codex (gpt-5.x, session `019f5c95`, 2026-07-14):** all three design
> judgments upheld (representation, per-driver multi-match, SQL PK-inference retirement) and every
> ground-truth claim confirmed `MATCHES`; Codex's concrete holes — an incomplete blast radius,
> preview surfacing not being free, `RowBatch` selector invariants, the gdrive `child_id` caveat,
> and the build-script exec scope — are folded into §3–§5 below.

### 1. State — one shared root cause, three faces

A write's effect node carries a **single** `RowBatch`, `EffectNode.args` (`plan/src/node.rs:115`),
and the core collapses `SET` and `WHERE` into it: `setwhere_row_batch` (`core/src/eval.rs:1588`)
pushes each `SET` assignment, then appends each `WHERE` `col == const` leaf **only if the column is
not already a SET column** (`eval.rs:1600`). A `WHERE` key that shares a `SET` column is dropped —
the selector `'Y'` in `SET name='X' WHERE name='Y'` is lost, leaving a bare `SET name='X'` the driver
cannot tell from "rename the container." The `WHERE` leaves that survive are only conjoined equality
constants (`collect_eq_constants`, `eval.rs:1613`); `OR` / non-eq / non-const are already dropped as
"no addressable key."

The three faces of the one bug:

- **gdrive UPDATE** (`driver-gdrive/src/effect.rs:394` `decode_move`) **refuses loudly** — the v0.0.60
  safe fix (was: silently renamed the folder). Workarounds that work: `/drive/id:<id>` renames the
  folder itself; a file rename by node path works.
- **gdrive REMOVE** (`effect.rs:491` `remove_target_id`) **already treats `args` as a selector** —
  `[]` → the path node; exactly `[name]` → the child of that name; richer → fail closed. It works
  only because REMOVE has no SET payload, so its `WHERE` columns *are* the args. UPDATE is the sole
  verb that conflates a SET payload and a WHERE selector in one batch.
- **SQL UPDATE** (`driver-sql/src/applier.rs:382` `split_update`) **ignores the `WHERE` clause
  entirely** and re-splits `args` by **primary-key membership** (key → WHERE via `build_key_where`,
  non-key → SET). `SET name='new' WHERE name='old'` (name not PK) dedups to `{name:'new'}`, finds no
  key → *rejected* "would update every row." Fail-closed, never silently wrong — but the legitimate
  rename-by-non-key-filter is unrepresentable, and SQL silently substitutes the PK for the operator's
  actual `WHERE`. The identical decision to gdrive's, papered over by a different applier convention.

`EffectNode` is `#[non_exhaustive]` with the explicit charter (`node.rs:110`): *"representation can
gain internal fields without a breaking grammar change."* The grammar is frozen; the plan
representation is free to grow. qfs is experimental — **no back-compat, no migration window; a hard
break is correct.**

### 2. Options

**A. Where the selector lives.**
- *A1 — a new field on `EffectNode`* (`selector: Option<RowBatch>`), populated by the lowering,
  read by every applier. General; blessed by `#[non_exhaustive]`; renders in preview for free (the
  node is "safe to render and log"). One representational concept, uniform across drivers.
- *A2 — a gdrive-local decode hack*: smuggle the selector as a reserved `__where__`-prefixed column
  inside the flat `args` (the `applier.rs:254` doc comment already gestures at this for SQL).
  **Rejected**: it re-creates the very conflation the bug is about (selector and payload sharing one
  schema), it is per-driver (SQL keeps its PK-inference; the next driver invents a third
  convention), and a magic column name is a latent collision with a real listing column.
- *A3 — split `args` into `set`/`where` at the parser AST already*: the AST already keeps them apart
  (`EffectBody::SetWhere { set, filter }`, `parser/ast.rs:389`); the loss is purely in the *plan*
  lowering. So the fix belongs at the plan boundary, not the grammar. A3 collapses into A1.

**B. Selector shape.** *B1 — a one-row `RowBatch` of `col == const` equalities* (exactly what
`collect_eq_constants` already yields) vs *B2 — a full predicate tree* (ranges, `IN`, `OR`). B2 is
YAGNI: the write lowering only ever produces conjoined equality constants; richer *read* predicates
already flow as the residual filter on the read path, not the write selector; and an applier must
resolve a selector down to specific node ids / rows, which only equality supports without a scan
contract. The `#[non_exhaustive]` node can grow a richer selector later if a real need appears.

**E. Adoption scope.** *E1 — gdrive-only* (SQL stays on PK-inference) vs *E2 — one uniform change*
(the lowering always routes `WHERE` → `selector` and `SET`/`VALUES` → `args`; gdrive and SQL both
read `selector` for the match). E1 leaves two conventions and the latent SQL shape unfixed; E2 is
the "smallest general change that also resolves the latent sql same-column shape" the ticket asks
for — and a *split* rule ("WHERE lives in args for REMOVE but in selector for UPDATE") is exactly the
per-verb inconsistency that produced this bug.

### 3. Recommendation — a general selector channel, uniform routing

**Add one field.** `EffectNode.selector: Option<RowBatch>` (a one-row batch whose schema columns are
the `WHERE` keys and whose row values are the equality constants), plus a `with_selector(RowBatch)`
builder. Reuse `RowBatch` — it is the owned type and already `Serialize`. **Invariant discipline
(Codex):** `RowBatch` itself permits arbitrary row counts and columns, so the selector shape —
*exactly one row, no duplicate or conflicting keys, equality constants only* — must be enforced by
the `with_selector` builder (or a validating constructor), not merely assumed. A dedicated
`Selector(RowBatch)` newtype that makes those invariants a type property is the stronger form and is
worth taking if the builder-discipline route feels leaky at implementation; the bare
`Option<RowBatch>` with a validating builder is the minimal form. Either way the invariant must be
*enforced*, not documented.

**Lower uniformly.** Retire the dedup. The `EffectBody::SetWhere` lowering (`eval.rs:952`) becomes:
`args` = the `SET`/`VALUES` payload only; `selector` = the `WHERE` equality leaves (`collect_eq_constants`),
**always**, for every write verb. `setwhere_row_batch` splits into `set_row_batch` (SET → args) and
`where_row_batch` (WHERE → selector); the same-column case now survives because the two live in
different fields. REMOVE's `WHERE` moves from `args` to `selector` too (uniformity), so `args` means
"the payload being written" across all four verbs and `selector` means "which existing rows/nodes."

**Consume.**
- *gdrive `decode_move`*: when a NAME-path folder UPDATE carries a `selector` with a single `name`
  key, resolve the child-by-name under the folder (the existing `WriteResolver` list/`existing`
  path, the same machinery `remove_target_id` uses) and emit `DriveEffect::Move { id, new_name }`
  renaming that child — instead of refusing. The refusal stays for the genuinely ambiguous case
  (no selector, or a selector that resolves to the container). **Caveat (Codex):** use an
  ambiguity-safe resolver — `resolve_node` (`driver-gdrive/src/read.rs:253`), which fetches up to two
  and returns `AmbiguousTarget` on ≥2 — **NOT** `child_id` (`read.rs:406`), which intentionally
  returns *any* hit for create-collision probing and would silently rename an arbitrary duplicate.
- *gdrive `remove_target_id`*: read the `[name]` key from `selector` instead of `args` — same
  behavior, now on the canonical channel.
- *SQL `lower_effect`*: build the `WHERE` from `selector` (honoring the operator's actual clause);
  `assignments` come from `args`. This **retires the PK-inference for the UPDATE/REMOVE match** — a
  deliberate semantic correction (SQL stops silently substituting the PK for the written `WHERE`).
  UPSERT `conflict_keys` still come from the PK (retry-safety is a separate concern, unchanged).

**Multi-match is a per-driver policy, not a global rule.** For **node-resolving** drivers (gdrive
and object stores — the effect must land on ONE object id): match 0 → `NotFound`; match 1 → proceed;
match ≥2 → refuse `AmbiguousTarget` (mirroring `resolve_node`/`remove_target_id`'s existing
fail-closed posture — refusing beats renaming the wrong node). For **relational** drivers (SQL): a
`WHERE` naturally addresses a *set* of rows — `UPDATE … WHERE` updates **all** matching rows, as SQL
does; there is no ambiguity to refuse. The selector channel is general; the "one object" constraint
is the node-resolver's, not the representation's.

**Preview & timing.** The `selector` should render in the effect preview so a COMMIT reads honestly
(`UPDATE /drive/my/x SET name='X' WHERE name='Y'`, not a bare `SET name='X'`). **This is NOT free
(Codex):** `PreviewRow` (`plan/src/preview.rs:19`) omits `args` today, so surfacing the selector is
explicit preview JSON/display work, not an automatic consequence of adding the field — budget it in
the implementation. The ambiguous-refusal for gdrive surfaces at **commit** (child resolution needs a
live list — a `WriteResolver` call, exactly like today's `existing`/`resolve_node` refusals); the
plan-time predicate type-check (`eval_write`, `eval.rs:896`) is unchanged.

**Composition — the working routes are untouched.** File-rename-by-path (no `WHERE` → `selector`
None), folder rename by `/drive/id:<id>` (id path, no selector), and folder moves via
`add/remove parents` (those are SET columns in `args`, distinct from any `WHERE`) all keep working;
`selector` is `None` for each. Only the previously-dropped same-column case changes behavior.

### 4. Blast radius (bounds the implementation ticket)

- `core/src/eval.rs`: split `setwhere_row_batch` → `set_row_batch` + `where_row_batch`; remove the
  dedup; the `EffectBody::SetWhere` lowering at `eval.rs:952` sets both `args` and `selector`;
  REMOVE's `WHERE` routes to `selector`.
- `plan/src/node.rs`: `EffectNode.selector: Option<RowBatch>` + `with_selector` builder (+ its
  `Serialize`/preview surfacing).
- `plan/src/preview.rs`: `PreviewRow` (line 19) has **no `args`/selector field today** — add explicit
  selector surfacing so previews read honestly. Not automatic (Codex).
- `driver-gdrive/src/effect.rs`: `decode_move` resolve-child-and-rename via selector, using the
  ambiguity-safe `resolve_node` (`read.rs:253`), **not** `child_id` (`read.rs:406`);
  `remove_target_id` reads selector; the multi-match `AmbiguousTarget` path.
- `driver-sql/src/applier.rs`: `lower_effect`/`split_update` build the `WHERE` from `selector`,
  retiring PK-inference for the UPDATE/REMOVE match (UPSERT conflict-keys unchanged). **Also the
  catalog `DROP TABLE` REMOVE** (`applier.rs:231`) reads `name` from `args` — migrate it too.
- **`driver-cf/src/effect.rs` (Codex — HIGH):** Cloudflare **D1 mirrors SQL** exactly — its own
  `split_update` (`:341`) + `build_key_where` (`:376`) + `key_columns` PK-inference — so the same
  WHERE-from-selector migration MUST land here in the same change; the **KV namespace REMOVE**
  (`:152`) reads `key` from `args` too.
- **Other appliers reading a `WHERE`/selector key out of `args` today (Codex audit — must migrate
  in the same PR or the uniformity claim is false):** `driver-gmail/src/effect.rs` (collection
  UPDATE/REMOVE read `id` from `args`, `:177`/`:207`), `driver-slack/src/effect.rs` (reaction/message
  REMOVE read `emoji`/`ts`, `:253`), `driver-transform/src/applier.rs` (REMOVE reads `name`, `:40`),
  `driver-sys/src/applier.rs` (`/sys/accounts` REMOVE reads `provider`/`account`, `:81`). Lower
  exposure: `driver-objstore` bucket-root REMOVE reads `key` from `args` (`:97`) but bucket root
  does not advertise `Remove` (backstop only).
- Golden snapshots: plan/effect goldens that print `args` for `SET…WHERE` or REMOVE will re-bless
  (`QFS_BLESS=1`); add the new same-column rename goldens.
- New tests: gdrive `SET name WHERE name` renames the child (single-match), refuses on ≥2 matches;
  SQL `UPDATE … WHERE <non-key>` now honors the real clause; the existing REMOVE-by-name recipe
  stays green on the moved channel.

### 5. Open questions / non-goals

- **Non-goal:** richer selectors (ranges/`IN`/`OR`). The channel is equality-conjunction only;
  grow the non-exhaustive node later if a real need lands.
- **Non-goal:** changing the grammar. The AST already separates `SET`/`WHERE`; this is a plan-lowering
  and applier change.
- **Decide at implementation:** whether the SQL/CF-D1 PK-inference retirement ships in the same PR or
  as an immediate fast-follow if the applier test surface proves large. Preference: same PR (it is the
  same decision), but it is the one acceptable split — gdrive (the headline bug) must not wait on it.
- **Audit — done (Codex):** the enumeration of every applier reading a `WHERE`/selector key out of
  `args` is complete and listed in §4 (gmail/slack/transform/sys/cf-KV/sql-catalog + the cf-D1
  mirror). Each moves to `selector` in the same PR or the uniformity claim is false — this is no
  longer an open question, it is a checklist.
- **Non-goal (with a caveat):** the optional `Driver::plan_write(path, verb, args)` hook
  (`driver/src/lib.rs:633`) takes no selector parameter. It does not block this bug (the affected
  drivers decode from the `EffectNode` directly, not through that hook), so it stays out of scope —
  but the "uniform for every write verb" claim is only fully true once `plan_write` also carries the
  selector. Flag for a later signature change, not this ticket.

### Status

Design gate cleared. `needs_design_brief` is satisfied; the remaining work is a normal
implementation ticket writable directly from §3–§4 above. The decision is recorded in the living
blueprint (§7 Runtime, "The selector channel", designed-not-yet-implemented).

## Resolution — increment 1 (2026-07-14, v0.0.63 → v0.0.64)

**The headline bug is fixed.** `UPDATE /drive/my/<folder> SET name='X' WHERE name='Y'` now renames the
matching child instead of the v0.0.60 safe refusal. Implemented as the **additive** first increment
of the brief (the brief's sanctioned split — the full applier surface is large):

- **`EffectNode.selector: Option<RowBatch>`** + a validating `with_selector` builder
  (`plan/src/node.rs`); `#[serde(skip_serializing_if)]` so no-`WHERE` effects keep their goldens.
- **Additive lowering** (`core/src/eval.rs`): the `WHERE` eq-leaves are populated onto `selector` via
  a new `where_selector_batch` (no de-dup, so a same-column key survives), while `args` stays exactly
  as `setwhere_row_batch` produced — so **every other applier is untouched and no golden churns**.
- **gdrive consumer** (`driver-gdrive/src/effect.rs`): a name-path folder `UPDATE` with a single
  `name` selector resolves the child ambiguity-safe (via `existing`/`resolve_node`, refusing
  `AmbiguousTarget` on ≥2 — **not** the collision-probe `child_id`, per the Codex caveat) and renames
  it; a non-`name` or absent selector keeps the loud refusal.
- **Preview surfacing** (`plan/src/preview.rs`): ` where <keys>` (key columns only, secret-free).

**Tests (all green):** `update_folder_set_name_where_name_renames_the_matching_child`,
`update_folder_where_name_ambiguous_child_refuses`, `update_folder_with_non_name_selector_still_refuses`
(gdrive), `update_lowers_the_where_into_the_selector_channel` (core). Full `cargo test --workspace`
green; the only golden movement was the `skip_serializing_if` fix (no-`WHERE` nodes unchanged).

**Deferred to increment 2** (follow-up ticket `20260714120000-effect-selector-uniform-migration.md`):
the *uniform* lowering (move `WHERE` off `args` entirely) and migrating every applier that reads a
filter from `args` (SQL + CF D1 PK-inference retirement, CF KV, gmail, slack, transform, sys,
sql-catalog) to read `selector`, plus the `plan_write` selector parameter. Blueprint §7 records both
increments. This ticket may be archived once increment 2 is scoped; the concern
`per-row-drive-folder-rename-needs` flips to resolved when increment 1 ships (the behavior it names —
the per-row folder rename — now works).
