---
created_at: 2026-07-06T12:05:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 2h
commit_hash: 48ba4e8
category: Added
depends_on: []
---

# Honor the per-MAP IRREVERSIBLE flag in the MAP write facet

## What's wanted

The declared-driver tier-1 -> tier-2 work landed view-body expansion and redirect confinement (on
branch work-20260705-173620), leaving one named park from concern 21: the per-map `IRREVERSIBLE`
flag is parsed and loaded but not enforced — a MAP marked IRREVERSIBLE does not gate its apply
through the irreversible-confirmation path. Wire the flag through the MAP apply facet so an
IRREVERSIBLE MAP write requires the same explicit confirmation as other irreversible operations.

## Approach correction (found during implementation)

The sketch said "thread the flag into the apply facet `RestApplyDriver`" — that is the WRONG layer.
The irreversible gate is **plan-time**: `core/eval.rs` sets `EffectNode::irreversible` (from an
inherent `REMOVE` or a `CALL`'s `ProcSig.irreversible`), and `plan.is_irreversible()` drives PREVIEW
surfacing + the `--commit-irreversible` floor. `RestApplyDriver` runs *after* that gate, so enforcing
there would leave PREVIEW blind and break the "PREVIEW always surfaces irreversible" contract. The
fix must set the plan-time bit. Owner approved this plan-time design (case A).

## Design (implemented)

Convey the declared MAP's irreversibility to the plan node through the describe mount, reusing the
existing gate:

1. New `Driver::write_irreversible(path, verb) -> bool` (default `false`, zero ripple) — the planner
   asks the routed driver whether a `(path, verb)` write is irreversible.
2. `core/eval.rs` ORs it onto the generic write node (`EffectNode::irreversible` OR-combines, so it
   never clears the inherent `REMOVE` bit).
3. `ResourceMap` gains `irreversible_verbs`; `RestDriver::write_irreversible` answers from it, reusing
   its own path→resource resolution.
4. `MountDriver` forwards `write_irreversible` (inbound path remap, like `plan_write`).
5. The declared describe mount's `resources()` lifts each `DeclaredMap.irreversible` onto the resource
   config; `#[allow(dead_code)]` removed.

## Key files

- `crates/driver/src/lib.rs` (`Driver::write_irreversible`), `crates/core/src/eval.rs` (OR onto the
  write node), `crates/driver-http/src/{config.rs,lib.rs}` (`ResourceMap.irreversible_verbs` +
  `RestDriver::write_irreversible`), `crates/qfs/src/mount_adapter.rs` (`MountDriver` forward),
  `crates/qfs/src/declared_driver.rs` (`resources()` lift + `DeclaredMap`).

## Considerations

- The other two tier-1 parks (view-body expansion, redirect confinement) landed via tier-2 on this
  branch — out of scope here.
- Source concern: `.workaholic/concerns/21-tier-1-declared-driver-scope-stops.md` (to be
  re-verdicted at /report: two of three named gaps resolved; this is the remainder).
