---
created_at: 2026-07-14T18:27:40+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort: 2h
commit_hash:
category: Changed
depends_on: [20260714182710-shell-face-slice1-ls-cat-describe-typed.md]
mission: language-design-review-layering-principles-and-semantic-gaps
needs_design_brief: false
---

# Shell face — `/type` catalog mount + `describe` REPL builtin

## Overview

Split from shell-face slice 1 (20260714182710) by capability boundary — the entry-kind-typed `ls`
defect fix shipped there; these two are **additive** completions of the navigation face, each a
non-trivial new surface. Ruled **adopt** by the owner (2026-07-14); see the shell-face brief §4
slice 1.

## Design (settled — shell-face brief §1.2 + §4 slice 1)

- **Mount `/type` as a read-only catalog** mirroring `/transform`
  (`crates/driver-transform/` is the ~734-line template): a **pure describe facet** (cred-free —
  `DESCRIBE /type/<name>` teaches the declared type's shape) plus a **System-DB-injected read facet**
  over `sys_drivers kind='type'` rows (the loaders `load_declared_types` / `load_declared_type_defs`
  in `crates/qfs/src/declared_driver.rs` already read them). Then `ls /type` = SHOW TYPES and
  `SELECT /type` list the declared types — making blueprint §5.4's "`ls /type` is SHOW TYPES"
  (docs/blueprint.md:274) true in the binary. Wire it in `crates/qfs/src/shell.rs` beside the
  `/transform` mount + its read facet (one System-DB open).
- **`describe` as a REPL builtin**: add `Builtin::Describe` (line-head ident, not a keyword), an
  `Outcome::Described` variant (or render through the existing describe path), and reuse
  `run_describe`'s machinery (`crates/exec/src/lib.rs:921`) so an operator can read a path's contract
  without leaving the session (today `describe` is one-shot CLI only). Bind the REPL rendering in
  `crates/qfs/src/shell.rs`.

## Key Files

- New `/type` mount: a describe facet (mirror `crates/driver-transform/src/lib.rs`) + a binary read
  facet (`crates/qfs/src/…`, injected from the System DB like `TransformReadDriver`).
- `crates/qfs/src/shell.rs` — register the `/type` mount + read facet; the `describe` REPL render.
- `crates/exec/src/shell/desugar.rs` — `Builtin::Describe` (pure navigation, `is_effect=false`).
- `crates/exec/src/shell/session.rs` — the `describe` builtin handler + `Outcome` variant.
- `crates/exec/src/shell/complete.rs` — add `describe` to the completer's builtin names.
- `docs/blueprint.md` — flip §5.4/§5.5 "`ls /type` = SHOW TYPES" and the §9 pending note to shipped.

## Considerations

- **Pure navigation** — `describe` builds no plan; the `/type` read facet fails closed with a
  structured read error when no System DB resolves (like the transform facet), while its cred-free
  describe still plans.
- `/type` is the **catalog/shell face**, never a reference site — the §5.5 lock (`of /type/x`,
  `create type /type/…` rejected) stays; this mount is only for `ls`/`describe`/`SELECT`.
- Re-version the plugin if the added navigation surface is taught (patch).

## Quality Gate

- `cargo test/clippy/fmt/gen-docs/gen-skills/check-migrations` green.
- New tests: `ls /type` = SHOW TYPES answers inside one session; `describe /type/<name>` and
  `describe /transform/<name>` render inside the REPL; the `/type` read facet fails closed with no
  System DB.

## Final Report

Development completed as planned. Both surfaces landed additively: `/type` mounts as a read-only
catalog (a new `qfs-driver-type` crate mirroring `qfs-driver-transform`'s pure/injected split, plus
the binary-side `src/type_catalog.rs` read facet), and `describe` became a REPL builtin
(`Builtin::Describe` → `Outcome::Described`) rendering through the SAME `Renderer::describe` the
one-shot path uses. Full gate green: 2477 tests, clippy/fmt/gen-docs/gen-skills/check-migrations all
exit 0. Bumped qfs 0.0.66 → 0.0.67 and all four plugin version fields 0.11.5 → 0.11.6.

### Discovered Insights

- **Insight**: A declared type has TWO spellings and they are not interchangeable: `sys_drivers.name`
  stores the key in **path** form (`/type/chatwork/message`), but the grammar accepts only the
  **name** (`of chatwork/message`) — `of /type/x` is a parse error, and the parser normalises the
  bare name into the path key before `declared_types().get(name)`.
  **Context**: This bit the first cut of this ticket. Rendering the stored `name` column verbatim
  made `ls /type` print the one spelling the language rejects — a catalog whose output could not be
  pasted into the reference site it exists to serve. The read facet now strips the mount prefix via
  `name_from_path`. Anything else reading `sys_drivers` `kind='type'` for a USER-facing surface owes
  the same translation; the §5.5 "paths are data, names are definitions" rule is not merely
  conceptual, it is a live encoding boundary between the store and the grammar.

- **Insight**: `/type`'s name is the WHOLE remainder after the mount, not the first segment — unlike
  `/transform`, whose `name_from_path` takes `split('/').next()`.
  **Context**: §5.4 lets a declared type nest (`chatwork/message`), so copying `/transform`'s
  first-segment rule would silently truncate every qualified name to its catalog (`chatwork`) and
  collapse distinct types onto one row. The two mounts look like twins and their name rules differ.

- **Insight**: `sys_drivers` is append-shaped — re-installing a type inserts a SECOND row with the
  same name, and every resolution path disambiguates with `ORDER BY id DESC` (newest wins).
  **Context**: A listing that did not collapse by name would show superseded declarations as if they
  were live, contradicting what `of <name>` actually resolves. The scan applies the same newest-wins
  rule. Any future catalog surface over these rows inherits this obligation.

- **Insight**: `/type` is the first driver crate that is NOT a `qfs-runtime` consumer (read-only ⇒ no
  applier ⇒ no `PlanApplierBridge`), so it belongs in `dep_direction.rs`'s binary allowlist but NOT
  in its runtime-leaf allowlist.
  **Context**: The two allowlists in that guard look parallel and every prior driver appeared in
  both; a read-only driver is the case that separates them.
