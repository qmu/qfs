---
created_at: 2026-07-14T18:27:30+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort: 4h
commit_hash:
category: Changed
depends_on: [20260714182710-shell-face-slice1-ls-cat-describe-typed.md]
mission: language-design-review-layering-principles-and-semantic-gaps
needs_design_brief: false
---

# Shell face slice 3 — mutation verbs (`cp`/`mv`/`rm`) get their per-entry-kind ruling

## Overview

The shipped mutating verbs desugar uniformly (`cp`/`mv` → `UPSERT`, `mv` also `REMOVE`; `rm` →
`REMOVE`) regardless of the destination's entry kind. That is wrong beyond blobs — the sharpest
case: **`mv` on a mail path is copy+delete = send a new message and trash the original.** This slice
rules the per-entry-kind semantics (all behind the EXISTING preview→commit gate — no new gate, no
new machinery). Ruled **adopt** by the owner (2026-07-14); see the shell-face brief §3 matrix + §4
slice 3.

## Design (settled — shell-face brief §3 entry-kind × verb matrix)

- **`cp` lowering keyed on the destination's entry kind** (known at desugar time from describe):
  **blob → `UPSERT INTO`** (idempotent, retry-safe — the shipped choice, keep it); **table / append
  log / object graph → `INSERT INTO`** (an idempotent "send" into an append log is a lie). This also
  makes the mission's "`cp` ≡ membership-checked `insert into`" literally true where the destination
  is an `OF`-typed table — membership continues to police via the shipped
  `materialize_pipeline_source` → `args` → `check_table_membership` chain (no new machinery; add a
  cross-service golden).
- **`mv` requires src and dst to be the SAME entry kind**, and lowers per kind:
  - **blob → blob**: copy+verify+delete as today, or the driver's native rename when
    `Verb::Update`/`Mv` caps offer it (Drive rename is an `UPDATE`).
  - **definition → definition, same catalog**: rewrite the catalog row's name — one previewed
    catalog write (rename-a-definition).
  - **everything else** (rows, append logs, cross-kind): a **structured refusal naming the honest
    spelling** (relabel is `UPDATE labels`; a row "move" is `UPDATE`/`REMOVE`+`INSERT`).
- **Definition-catalog verbs** (`/type`, `/transform`): `cp` = clone, `mv` = rename, `rm` = drop —
  ordinary previewed catalog writes (`rm /transform/<name>` already ships irreversible-gated;
  clone/rename are small). Add the **categorical plan-time refusal of a data-row → definition-catalog
  `cp`** (`category_error`, the §5.5 line — the two categories never pool).
- **`mkdir`: defer** (recorded, not silent). Only Drive has real folder semantics and its
  folder-MIME `INSERT` form exists; S3/R2 prefixes are virtual and tables/defs have dedicated
  constructors (`CREATE TABLE`/`CREATE TYPE`) — a verb that is a category error on four of five
  entry kinds does not earn the slot yet.

## Key Files

- `crates/exec/src/shell/desugar.rs` — `cp`/`mv`/`rm` desugar keyed on entry kind (from describe);
  the `mv` same-kind rule + honest-spelling refusals; def-catalog clone/rename/drop.
- `crates/core/src/eval.rs` / `crates/core/src/resolve.rs` — the plan-time `category_error` for a
  data-row write into a definition catalog path.
- `crates/qfs/src/sql_contracts.rs` / `crates/qfs/src/commit.rs` — confirm the membership seam
  (`materialize_pipeline_source` → `check_table_membership`) polices a `cp` into an `OF` table.
- `docs/blueprint.md` — the §9 verb-semantics matrix (or a pointer to the brief) if not fully
  captured in slice 1.

## Considerations

- **All mutation rides the shipped gate** — a `cp`/`mv`/`rm` builtin previews by default and applies
  only on the typed `COMMIT` (REPL) / `--commit` (one-shot); the irreversible floor still applies.
  This slice adds NO new gate and NO plan-node type.
- **`mv` is the only verb without a uniform meaning** — the same-kind rule is the discipline; do not
  paper over the mail trap with a silent copy+delete.
- Open sub-questions (owner-recommended defaults, confirm at implementation): (1) `mv` on an append
  log = flat refusal; (3) `cp` def-clone across catalogs = pure refusal.
- Experimental repo — hard breaks are correct; re-bless any effect/preview goldens that snapshot the
  old uniform desugar.

## Quality Gate

- `cargo test/clippy/fmt/gen-docs/gen-skills` green; goldens re-blessed with each diff eyeballed.
- New tests: `cp` into a blob = UPSERT, into a table/log = INSERT; `cp` into an `OF` table is
  membership-checked; `mv` blob→blob works, `mv` on mail refuses naming `UPDATE`, `mv` def→def
  renames; data-row → `/type`/`/transform` `cp` is `category_error`; def-catalog clone/rename/drop.
- Plugin re-versioned (minor if the taught mutation surface changes semantics).

## Final Report

Implemented as designed, with ONE ruled deviation the owner approved at implementation time (below).
`cp` is keyed on the destination's entry kind (blob → `UPSERT`, else `INSERT`); `mv` is same-kind-only
(blob→blob copy+delete, everything else a structured refusal naming the honest spelling);
`NodeDesc::category` (`Data` | `Definition`) landed as the driver-stated §5.5 signal and makes a
data-row → definition-catalog `cp` a `category_error`. All behind the shipped preview→commit gate — no
new gate, no new plan-node type. `rm` needed no ruling. `mkdir` stays deferred. Full gate green: 2493
tests, clippy/fmt/gen-docs/gen-skills/check-migrations all exit 0. qfs 0.0.69; plugin 0.11.7 (patch,
not minor — see the taught-surface insight below).

**Deviation (owner-approved):** the ticket's def-catalog `cp` = clone and `mv` = rename are
**inexpressible**, so both refuse instead. See the first insight.

### Discovered Insights

- **Insight**: Def-catalog clone and rename cannot be expressed, and the reason is structural, not
  effort. A definition row **carries its own name**, so `cp /transform/a /transform/b` lowers to an
  INSERT of a row still named `a` — re-inserting `a`, not cloning it to `b`. A rename needs an
  in-place name rewrite that neither catalog offers: `/type` exposes **no write verb at all** (slice
  1's own ruling — a type is installed through `/sys/drivers`), and `/transform` has no `UPDATE`
  (install/uninstall only, by design). Both now refuse, naming the one spelling that works
  (re-declare under the new name, then remove the old).
  **Context**: The ticket's "one previewed catalog write" assumed a write verb that does not exist.
  Expressing it needs a name-rewriting projection the shell cannot build — a real follow-up if
  def-rename is wanted. The refusal is the honest floor: a silent copy+delete would leave every
  reference to the old name dangling, which is the same class of harm as the mail trap.

- **Insight**: The plugin bump is a PATCH, not the minor the ticket anticipated — because the skills
  teach exactly ONE mutation, `cp /local/… /drive/…`, and `/drive` describes as `blob_namespace`, so
  it still lowers to `UPSERT`. The taught surface's semantics are unchanged.
  **Context**: The rule is "minor if the TAUGHT surface breaks", not "minor if the surface changes".
  Enumerating what the skills actually teach (`grep -ohE "(cp|mv) /… /…"` over `SKILL.md` +
  `docs/cookbook`) answers it in one command, and is worth doing rather than assuming the worst.

- **Insight**: The interactive shell's `/local` **reads from the process cwd but writes to the
  filesystem root** — `shell.rs` roots the READ mount at `current_dir()` while `commit.rs:246` roots
  the apply driver at `/`. So `mv a.md b.md` in the REPL previews correctly against `$CWD/a.md`, then
  COMMITs into `/b.md` (observed: `PermissionDenied` — and it would SUCCEED as root).
  **Context**: Pre-existing and untouched by this slice (blob→blob lowering is byte-identical to
  before), found only by driving a real COMMIT rather than trusting the preview. It means no `cp`/`mv`
  COMMIT in the REPL has ever worked as the operator reads it. Deserves its own ticket; recorded as a
  concern.

- **Insight**: `Archetype` and `NodeCategory` are both `#[non_exhaustive]`, so matching on the pair
  needs a wildcard arm — which is a design prompt, not a chore: the wildcard decides what an
  unmodelled entry kind does. `mv` sends it to a refusal, so a future archetype can never silently
  inherit copy+delete.
  **Context**: The same reasoning gives every unknown/undescribable path the shipped fallback rather
  than a guess: `ls` → bare read, `cp` → `UPSERT`, `mv` → copy+delete. New rules apply only where the
  driver actually spoke.
