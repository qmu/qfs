---
created_at: 2026-07-18T12:01:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: 20260718003100-describe-declares-a-rows-child-address.md
mission:
---

# enumerate: `ls` of a table node lists row addresses (the 番地 closure's second observation)

## Overview

Filed as the follow-up the settled 番地 design anticipated (strategy `plan.md`「閉包の原理」,
2026-07-18): every address answers **describe / enumerate / read**. The describe half and the
`@` selection lowering shipped with ticket `20260718003100` (branch `work-20260718-113000`,
v0.0.80): describe now declares `child_address` (key columns / entry name / none), and
`/x/@A` lowers once — in `qfs_core::plan` — to `|> where <declared key> == A`, for every read
consumer. `describe /x/@A` also resolves (the row-node view).

**enumerate is deliberately NOT built there.** `ls` of a table node should list **row
addresses** — the read projected to (address, label), same source, default order, and limit as
read — so the REPL's `ls`, the viewer's column, and a REST listing are the same observation,
and `cd /mail/INBOX` then `ls` shows mail addresses with `cd @<id>` entering one. That is
larger plumbing than the describe/lowering pair:

- The REPL `ls` desugar (`crates/exec/src/shell/desugar.rs:206`) currently keys on archetype
  only (blob projection vs bare read); an address projection needs the `child_address`
  declaration threaded into `NodeFacts` and a rendering of `@`-spelled addresses.
- `ls /sys` erroring today (measured 2026-07-17, the stopped describe-surface ticket) shares
  this root: interior catalog nodes have no row-address projection.
- `cd @<id>` (entering a row) touches the shell cwd model, which today holds containment
  segments only.
- The viewer column and REST listing want the same projection server-side (one lowering, no
  per-face gatekeeper — the drift this design exists to kill).

## Policies

- `workaholic:implementation` / `anti-corruption-structure.md` — the address→statement
  conversion stays inside qfs (one lowering site); no per-face gatekeeper in the REPL,
  viewer, or REST layer.
- `workaholic:implementation` / `objective-documentation.md` — `ls` must list only addresses
  that actually resolve (describe/read), never a rendered link the binary refuses.
- `workaholic:design` / `self-explanatory-ui.md` — a node that declares no child enumerates
  honestly empty, not as an error or a dead link.

## Key Files

- `packages/qfs/crates/driver/src/lib.rs` — `ChildAddress` (shipped; the declaration to render)
- `packages/qfs/crates/core/src/plan.rs` — the `@` lowering site (shipped; `cd @<id>` composes on it)
- `packages/qfs/crates/exec/src/shell/desugar.rs` — `ls` desugar, the first consumer to convert
- `packages/qfs/crates/exec/src/shell/session.rs` — `NodeFacts` / cwd model for `cd @<id>`

## Quality Gate

**Acceptance criteria**

- `ls` of a node declaring `child_address: key` lists `(address, label)` rows whose addresses
  are `@`-spelled and each answers `describe` and `read` (closure proven by test).
- `ls /sys` stops erroring: interior nodes enumerate their children.
- `cd @<id>` enters a row; `ls` inside shows the row's view (or an honest empty).
- Blob/EntryName nodes keep their current `ls` (name segments; no `@` rendering).
- Full gate green with bare exit codes; every new test watched red first.

## Considerations

- Do not add a second lowering: the address→statement conversion must reuse
  `qfs_core::plan`'s selection lowering (`cd @A` extends the pipeline, plan.md's one-move rule).
- Relation segments (`/@<id>/thread`) stay out of scope — a later phase behind the
  relation-metadata layer.
