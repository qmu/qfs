---
created_at: 2026-07-14T12:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
commit_hash:
category: Changed
depends_on: 20260713195008-effect-selector-channel-folder-rename.md
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
needs_design_brief: false
---

# Effect selector channel — increment 2: uniform lowering + migrate every applier off args-as-filter

## Overview

Increment 1 (ticket `20260713195008`, shipped) added `EffectNode.selector: Option<RowBatch>`
**additively** — the `WHERE` is populated onto `selector` while `args` stays exactly as before, so the
headline gdrive folder-rename bug is fixed with zero regression and zero golden churn, but the
`WHERE` still *also* lives in `args` (deduped) and every other applier still reads its filter from
`args`. That leaves the two-convention state the design brief wanted to retire.

Increment 2 makes the lowering **uniform**: the `WHERE` moves **off `args` entirely** onto `selector`
for every write verb, and every applier that today reads a `WHERE`/filter key out of `args` migrates
to read `selector`. This is the "smallest general change that also resolves the latent SQL/CF-D1
same-column shape" the brief committed to — deferred from increment 1 only because the applier +
golden surface is large (the brief's sanctioned split).

## Design

The decision is already made and recorded — **blueprint §7 "The selector channel"** and the design
brief on ticket `20260713195008` (Codex-reviewed). No new design work; this is the mechanical
migration. `needs_design_brief: false`.

## Key Files (the migration checklist, from the Codex-audited blast radius)

- `packages/qfs/crates/core/src/eval.rs` — `setwhere_row_batch` becomes SET-only (drop the WHERE
  append + its de-dup); `where_selector_batch` already produces the selector. Confirm REMOVE (empty
  SET + filter) now carries an **empty `args`** and a populated `selector`.
- `packages/qfs/crates/driver-sql/src/applier.rs` — `lower_effect`/`split_update`/`build_key_where`:
  build the UPDATE/REMOVE `WHERE` from `selector` (retire PK-inference for the *match*; UPSERT
  `conflict_keys` stay PK-based); catalog `DROP TABLE` REMOVE (`~:231`) reads `name` from `selector`.
- `packages/qfs/crates/driver-cf/src/effect.rs` — D1 mirrors SQL (`split_update ~:341`,
  `build_key_where ~:376`) — same migration; KV namespace REMOVE (`~:152`) reads `key` from `selector`.
- `packages/qfs/crates/driver-gmail/src/effect.rs` — collection UPDATE/REMOVE read `id` (`~:177`/`~:207`).
- `packages/qfs/crates/driver-slack/src/effect.rs` — reaction/message REMOVE read `emoji`/`ts` (`~:253`).
- `packages/qfs/crates/driver-transform/src/applier.rs` — REMOVE reads `name` (`~:40`).
- `packages/qfs/crates/driver-sys/src/applier.rs` — `/sys/accounts` REMOVE reads `provider`/`account` (`~:81`).
- `packages/qfs/crates/driver-gdrive/src/effect.rs` — `remove_target_id` (`~:497`) reads the `[name]`
  filter from `selector` instead of `args` (decode_move already uses `selector`).
- `packages/qfs/crates/driver/src/lib.rs` — the optional `plan_write(path, verb, args)` hook
  (`~:633`) gains a `selector` parameter so the "uniform for every write verb" claim is fully true
  (the git driver is the current consumer; audit whether it needs the selector).
- `packages/qfs/crates/plan/src/preview.rs` — already surfaces the selector; verify REMOVE previews
  still read honestly once `args` is empty.

## Considerations

- **This is a hard break with golden churn** (experimental repo — hard breaks are correct). Effect/
  plan goldens that snapshot a filtered UPDATE/REMOVE's `args` will move the WHERE key from `args`
  to `selector`; re-bless with `QFS_BLESS=1` and eyeball each diff.
- **Migrate every site in ONE PR** — a half-migrated state (some appliers read `selector`, some read
  `args`) breaks the ones not yet migrated the moment the lowering stops populating `args`. The
  checklist above is exhaustive per the Codex audit; if `grep` finds a new `node.args` filter read,
  add it.
- **SQL/CF-D1 semantic change:** honoring the real `WHERE` instead of inferring it from the PK is the
  correct behavior, but it changes what a filtered UPDATE/REMOVE matches — add tests feeding a
  non-key `WHERE` and a same-column `SET x WHERE x`.
- Keep UPSERT `conflict_keys` PK-based (retry-safety is a separate concern from the match filter).

## Quality Gate

- Full `cargo test --workspace` green (with re-blessed goldens); `clippy`/`fmt`/`gen-docs`/`gen-skills`
  clean. New tests: SQL non-key `WHERE` honored; a same-column `SET x WHERE x` round-trips through
  every migrated driver; REMOVE carries an empty `args` + populated `selector`.
- No `node.args`-as-filter read remains in any applier (grep-assert, or extend the exec-inventory-style
  governance if warranted).

## Final Report

Development completed as planned; the whole checklist migrated in one PR as the ticket required.
`setwhere_row_batch` is SET-only, so `args` is purely the payload and `selector` purely the match; a
REMOVE's `args` is now genuinely empty (an empty `RowBatch`, not one empty row). Every checklist site
moved to the new `EffectNode::selector_value`/`selector_text` helpers: SQL + CF D1 (PK-inference
retired for the match; UPSERT `conflict_keys` stay PK-based), CF KV, gmail, slack, transform, sys,
gdrive, and the sql-catalog `DROP TABLE`. `Driver::plan_write` gained a `selector` parameter. Full
gate green: 2497 tests, clippy/fmt/gen-docs/gen-skills/check-migrations all exit 0. qfs 0.0.70; no
plugin bump (see the taught-surface insight).

### Discovered Insights

- **Insight**: The most dangerous moment was the suite going **fully green**. Immediately after the
  lowering stopped populating `args`, all 2493 tests passed — while `update /mail/<label> … where id
  == '<msgid>'` was already broken (gmail read `id` via `text_col`, which reads `args`). The tests
  missed it because they **hand-build `EffectNode`s** with `with_args`, bypassing the lowering
  entirely, so they tested the applier against a convention the evaluator no longer produced.
  **Context**: This is the exact failure the ticket warned about ("a half-migrated state breaks the
  ones not yet migrated the moment the lowering stops populating `args`") — but the warning implies
  tests would *catch* it, and they cannot. The migration had to be driven by **auditing the code**,
  not by chasing red tests. Generalises: wherever a test constructs the IR directly instead of going
  through the lowering, it pins the applier's contract but proves nothing about the producer's. The
  two eval-level governance tests added here (a filtered UPDATE's `args` is SET-only; a REMOVE's
  `args` is empty) close that specific gap at the source.

- **Insight**: A blanket find/replace across a test file is unsafe here, because the SAME column name
  is a filter in one effect kind and a payload in another. Rewriting `("emoji", …)` / `("ts", …)`
  from `with_args` to `with_selector` silently broke slack's `INSERT` (emoji IS the reaction payload)
  and its `CALL slack.pin`/`unpin` (ts IS a literal procedure argument) — 5 tests, caught only by
  re-auditing each site against its `EffectKind`.
  **Context**: The selector/args split is per-(kind, column), never per-column. A `CALL`'s arguments
  and an `INSERT`'s payload legitimately live on `args` forever; only an `UPDATE`/`REMOVE` match key
  moves. Any future sweep must key on the effect kind, not the column name.

- **Insight**: The plugin needs no bump: all three filtered writes the skills teach behave
  identically. Two use `LIKE` (`remove /mail/inbox where subject LIKE '%spam%'`), which yields no
  equality key and therefore no selector — the same documented refusal as before; and `update
  /sql/pg/orders set status = 'shipped' where id == 7` lowers to identical SQL (previously via
  PK-inference on `id`, now via the real `WHERE`).
  **Context**: The PK-inference retirement is a semantic change that only shows on shapes the skills
  do NOT teach — a non-key `WHERE` (previously ignored) and a same-column `SET x WHERE x` (previously
  inexpressible). Checking the taught corpus rather than assuming a break is what makes this a
  no-bump instead of a minor.

- **Insight**: Dropping the `WHERE` from `args` left REMOVE with an empty payload, which broke the
  CF/D1 lowering's `single_row(node)?` — it demanded a row **before** dispatching on the verb, so a
  legitimately payload-free REMOVE failed as "carries no row payload".
  **Context**: A hidden coupling worth remembering: "every write effect has a row" was an invariant
  the old lowering accidentally guaranteed (the WHERE always put something in `args`). Retiring the
  dual convention makes "writes nothing ⇒ empty args" real, and any code asserting a payload up-front
  must move that assertion into the verbs that actually write.
