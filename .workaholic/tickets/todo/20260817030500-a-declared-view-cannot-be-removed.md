---
created_at: 2026-08-17T03:05:00+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
---

# A declared view cannot be removed, so a mistaken `CREATE VIEW` is permanent in local config

## Overview

`CREATE VIEW` has no inverse. Once a declared view (or type, or map) is committed to
`/sys/drivers`, nothing supported removes it:

```
$ qfs run "DROP VIEW /chatwork/rooms_raw"
RESERVED_AS_IDENTIFIER            -- fails to parse

$ qfs run "remove /sys/drivers where name == '/chatwork/rooms_raw'"
UnsupportedVerb { path: "/sys/drivers", verb: "REMOVE", supported: ["SELECT","INSERT"] }
```

So an experimental or mistaken declaration is permanent in local config. Observed 2026-07-27 while
iterating on the shipped `chatwork.qfs`: two scratch views created as workarounds are still in the
System DB with no supported way to clean them up.

This is the inverse-verb half of ticket `20260728085253` (a declared driver is undiscoverable
through `describe`), split out of it because the two share only their provocation. That ticket's
scope is the READ surface — what `describe` reports about a declared node and its children — and it
shipped without touching the declaration lifecycle. This one is a **grammar + catalog** change: a
new statement form and a `REMOVE` capability on a `/sys` catalog that deliberately has none today.

## Scope

1. An inverse for each declared-surface `CREATE` form, or one form that covers them
   (`DROP VIEW /<path>` / `DROP TYPE <name>` / `DROP MAP <verb> /<path>` / `DROP DRIVER <name>`).
   `VIEW` is a reserved keyword, so the parse failure above is a grammar gap, not a typo.
2. Decide what removing a *driver* means for the rows that mount under it — cascade, or refuse
   while any view/map still names the driver. Refusing is the safer default; state the choice.
3. Whether the `/sys/drivers` catalog gains `REMOVE` at all, or the DROP form desugars to a
   supported write. Granting a blanket `REMOVE` on a `/sys` catalog is the bigger blast radius of
   the two — a declared-surface DDL that removes exactly the row it names is narrower.
4. The removal is an ordinary previewed local write (zero network), gated like any other write.
   A driver currently CONNECTed at a binding path is the interesting case: removing its declaration
   leaves a binding pointing at nothing, which the two-source registry should report rather than
   silently serve.

## Key files

- `packages/qfs/crates/parser/src/grammar.rs` — `create_declared_view_stmt` / `create_map_stmt` /
  the `CREATE TYPE` form and their `insert_sys_drivers` desugar; the inverse belongs beside them.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — `load_declared_drivers` / `assemble`, whose
  newest-row-per-key resolution is what a removal has to interact with.
- `packages/qfs/crates/driver-sys/src/` — the `/sys/drivers` capability set that answers
  `SELECT`/`INSERT` today.

## Quality Gate

- A declared view/type/map created in a test System DB can be removed by a supported statement, and
  a subsequent `load_declared_drivers` no longer carries it.
- The removal previews before it commits, like every other write, and performs zero network I/O.
- Removing a driver with live view/map rows behaves as item 2 decides, pinned by a test either way.
- `cargo test --workspace` green.

## Considerations

Blast radius is the whole point of the caution here: `/sys/drivers` is config, and the newest-wins
resolution means a re-install already *supersedes* a declaration. Removal is for the case
supersession cannot reach — a scratch node whose path nothing will ever re-declare.
