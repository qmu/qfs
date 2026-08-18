---
created_at: 2026-07-29T16:30:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on:
mission:
---

# A declared view cannot be removed, so a mistaken declaration is permanent in local config

## Overview

`CREATE VIEW` has no inverse. Neither of the two routes a caller would try works:

```
$ qfs run "DROP VIEW /chatwork/rooms_raw"
RESERVED_AS_IDENTIFIER            # does not parse

$ qfs run "remove /sys/drivers where name == '/chatwork/rooms_raw'"
UnsupportedVerb { path: "/sys/drivers", verb: "REMOVE", supported: ["SELECT","INSERT"] }
```

So an experimental or mistaken declaration is **permanent**. Every declaration row is
append-only, and `assemble` resolves newest-per-`(kind, name, verb)` — which supersedes a
redeclaration but can never retract one.

**Observed, not speculated.** While implementing ticket 20260728085253 (declared-driver
discoverability) this run read the developer's live `/sys/drivers` and found three leftover
scratch declarations still resolving:

```
view  /chatwork/rooms_raw
view  /chatwork/rooms/{room}/messages_all
map   /chatwork/rooms/{room}/messages_form
```

They were created while working around that ticket's gaps and have no supported way to be
cleaned up. The discoverability work makes this **visibly worse**: `describe /chatwork` now
enumerates its children, so those three appear in the mount's advertised surface —

```
children:
  /chatwork/rooms
  /chatwork/rooms_raw          ← a scratch view nobody can delete
```

— which is exactly the case the walkable-surface feature exists to make trustworthy. A surface
that lists undeletable debris teaches an agent paths the operator does not want offered.

## Scope

1. An inverse verb for each declaration form: minimally `DROP VIEW <path>` / `DROP MAP <path>`,
   and by symmetry `DROP TYPE`, `DROP DRIVER`, `DROP SQL`. Decide whether one `DROP <kind>
   <name>` form covers all five or each is its own rule; the parser already has the five
   `CREATE` rules to mirror (`crates/parser/src/grammar.rs`, `driver_row_values`).
2. Decide the **storage** semantics and state them: a tombstone row (`kind='drop'`) that
   `assemble` honours, versus a real `DELETE` from `sys_drivers`. The append-only ledger and the
   `sys_ddl_events` audit chain argue for a tombstone; an operator's "clean it up" intent argues
   for the delete. Whichever is chosen, the row must leave an audit event.
3. `DROP` of a driver that other rows mount under (`chatwork` with live views) must be refused
   with a structured, pointed error naming what still depends on it — never a silent orphaning
   that leaves views resolving against a dead driver.
4. Dropping a declaration that a `path_binding` CONNECT still targets: refuse and name the
   binding, or require `DISCONNECT` first. State the rule; do not leave it implicit.
5. `RESERVED_AS_IDENTIFIER` on `DROP VIEW` is a second, independent defect — the parse error
   names the wrong problem. Whatever the outcome of item 1, an unsupported `DROP` must fail with
   a message that names the missing capability, not a lexer complaint about a reserved word.

## Key files

- `packages/qfs/crates/parser/src/grammar.rs` — the five `CREATE …` desugar rules and
  `driver_row_values` / `DRIVER_DECL_COLUMNS`; an inverse rides here.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — `assemble` (newest-per-key resolution) is
  where a tombstone would be honoured; `load_declared_drivers` / `load_declared_types` /
  `load_declared_sql_resources` all read the same table.
- `packages/qfs/crates/driver-sys/src/` — `/sys/drivers`' capability set (`SELECT`,`INSERT`
  today); a `REMOVE` route would widen it.
- `packages/qfs/crates/store/src/ddl_events.rs` — the audit chain a drop must land in.

## Policies

- workaholic:implementation / honest-surfaces — `describe` now advertises a declared mount's
  children, so anything undeletable in that list is taught to every agent that reads it. A
  surface is only trustworthy if the operator can curate what it offers.
- workaholic:design / 「推測するな、宣言して拒否せよ」 — a `DROP` that would orphan dependent views
  or a live CONNECT must refuse with a structured error naming the dependency, never guess at
  cascade semantics.
- workaholic:implementation / observability — a retraction is a config event; it lands in
  `sys_ddl_events` like every other declaration write, so the ledger stays a complete account.

## Quality Gate

- `DROP VIEW <path>` (and the sibling forms decided in scope item 1) parse, preview, and commit;
  after committing, the dropped node no longer appears in `describe`'s `children`, no longer
  resolves for `run`, and no longer loads via `load_declared_drivers`.
- The three leftover scratch declarations named in the Overview are removable by the shipped
  verb — exercised by a test that installs them, drops them, and asserts they are gone from both
  the describe surface and the loader.
- A `DROP` that would orphan a dependent view, or that targets a driver a `path_binding` still
  CONNECTs, is refused with a structured error naming the dependency (pinned by a test).
- An unsupported/misspelled `DROP` fails with an error naming the missing capability, not
  `RESERVED_AS_IDENTIFIER`.
- The drop lands in `sys_ddl_events`.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Considerations

- **Experimental, so a hard break is fine** — no migration or deprecation period is owed for the
  storage decision in scope item 2.
- The ticket that provoked this (20260728085253) deliberately did **not** fix it: an unqueued
  fix would have ridden into a commit whose message describes discoverability, and the
  storage-semantics decision in item 2 is a real design choice, not a mechanical addition.

## Notes

Provoked by ticket
`20260728085253-declared-driver-undiscoverable-through-describe.md`, whose own "Related: a
declared view cannot be removed" section first recorded the gap. Filed as its own ticket rather
than left as prose, because the corpus only carries tickets.
