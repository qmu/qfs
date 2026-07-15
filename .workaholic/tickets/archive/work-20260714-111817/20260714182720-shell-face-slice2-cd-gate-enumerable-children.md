---
created_at: 2026-07-14T18:27:20+09:00
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

# Shell face slice 2 — `cd` gate becomes "enumerable-children", not two archetypes

## Overview

The shipped `cd` gate (`namespace_check`, `crates/exec/src/shell/session.rs`) admits only
`Archetype::BlobNamespace | ObjectGraphWorkflow`. But `/sql/<conn>` (the table catalog),
`/transform`, `/sys`, and the mail/slack label/channel trees all describe as `RelationalTable`
(or gmail's root as `AppendLog`) — so `cd /sql/erp`, `cd /transform`, `cd /mail` are all wrongly
refused as `not_a_namespace`, while their `ls` (fixed in slice 1) is perfectly meaningful. The gate
conflates "a node whose children are locations" with two specific archetypes. Ruled **adopt** by the
owner (2026-07-14); see the shell-face brief §4 slice 2.

## Design (settled — shell-face brief §3 + §4 slice 2)

- **Replace the archetype pair with an "enumerable-children" predicate**: a node is enterable iff
  its **children are locations** (blob namespaces, object graphs, catalog interiors — `/sql/<conn>`,
  `/transform`, `/type`, `/sys`, `/server`, and the mail/slack label/channel trees), NOT when it is a
  row-bearing leaf. Keep **refusing `cd` into a row-set** (rows are values, not locations).
- The predicate is **data the pure describe registry serves** — either key on the node's capability
  to `Ls` a child-of-locations relation, or add a `NodeDesc` boolean the describe facet states. Do
  not invent shell-side heuristics; the driver's describe is the source of truth.
- Where an interior node **misdescribes** (gmail's root/label node as `AppendLog` when it is a
  navigable label tree — `crates/driver-gmail/src/lib.rs`), fix the **driver's describe**: that is a
  §5.1 driver-conformance correction the typed-path reading demands regardless of the shell (open
  sub-question 2, owner-recommended to make the correction).

## Key Files

- `crates/exec/src/shell/session.rs` — `namespace_check` → the enumerable-children predicate.
- `crates/driver/src/lib.rs` — if a `NodeDesc` boolean is chosen, add it to the describe contract.
- `crates/driver-gmail/src/lib.rs` (+ any driver whose navigable interior misdescribes) — describe
  the label/channel tree as navigable.
- `crates/exec/src/shell/path.rs` — confirm `resolve` + `..` clamping still composes with the wider
  set of enterable nodes.

## Scope finding (verified 2026-07-14 against the running binary)

`describe /transform` (a navigable catalog interior — `cd /transform` SHOULD succeed) reports
`archetype: relational_table` — the **same archetype** as `describe /sys/drivers` (a leaf table —
`cd` should refuse). So **archetype alone cannot distinguish a navigable catalog interior from a
row-bearing leaf**: the enumerable-children predicate needs a **per-node navigability signal**, not
an expanded archetype allowlist. Concretely this means adding a field to the `NodeDesc` describe
contract (`crates/driver/src/lib.rs`, `#[non_exhaustive]` — add a builder, e.g. `navigable(bool)`,
defaulting from the archetype so blob/object-graph nodes stay navigable with no change) and updating
each driver's `describe` to set it per path where the archetype default is wrong (the `/sql/<conn>`
catalog interior vs its `/sql/<conn>/<table>` leaves; `/transform`, `/type`, `/sys`, `/server`
roots vs their leaf rows; the mail/slack label/channel trees). **This is a driver-contract change
across many drivers — materially larger than the original 2h estimate.** Effort re-estimated to 4h.

## Considerations

- **Still pure navigation** — `cd`/`pwd` build no plan; this slice never touches the gate.
- A **driver describe change is a conformance fix**, not a shell hack — cover it with a driver test.
- Keep `..` clamped at the mount root (never cross a driver via relative paths); absolute
  cross-driver `cd` stays free.
- Open sub-question 2 (mail label `cd`): the owner leaned to make the gmail describe correction here.

## Quality Gate

- `cargo test/clippy/fmt` green; new tests: `cd /sql/<conn>`, `cd /transform`, `cd /type`,
  `cd /mail` (its label tree) succeed; `cd` into a row leaf still refuses `not_a_namespace`.
- Driver-conformance test for any corrected describe.

## Final Report

Development completed as planned, with the scope finding confirmed exactly: the gate needed a
per-node signal, not a wider archetype allowlist. `NodeDesc` gained a `navigable` field with a
`navigable(bool)` builder defaulting from the archetype (`navigable_by_default`), so every existing
`NodeDesc::new` call site is unchanged and the default reproduces the old gate's behaviour verbatim
(`BlobNamespace | ObjectGraphWorkflow` = enterable). `namespace_check` now reads that field.
Drivers opting in per path: `/transform` and `/type` (root navigable, item leaf), `/sql/<conn>`
(catalog navigable, table leaf), and gmail's label tree (`/mail` + `/mail/<label>`). Full gate green:
2483 tests, clippy/fmt/gen-docs/gen-skills/check-migrations all exit 0. Bumped qfs 0.0.67 → 0.0.68
(no plugin bump — no skill teaches `cd` into these paths).

### Discovered Insights

- **Insight**: The gmail describe correction is NOT an archetype change, and making it one would
  regress slice 1. The ticket called the root "`AppendLog` when it is a navigable label tree", which
  reads as "change the archetype" — but slice 1 made `ls` archetype-typed, so a `BlobNamespace` root
  would lower `ls /mail` to the blob `name/size/is_dir/modified` projection and fail against the
  label schema (`name` only). The archetype was already RIGHT (mail rows are an append log); only the
  navigability was missing.
  **Context**: Generalises past gmail — **archetype and navigability are orthogonal axes** and the
  contract now says so. "What shape are this node's rows" (drives `ls`, the schema, pushdown) is a
  different question from "are this node's children locations" (drives `cd`). Conflating them is the
  root cause of both this slice's defect and the temptation to "fix" it in the wrong place.

- **Insight**: Three of the ticket's named targets were narrower than assumed, for one shared reason:
  the node does not exist. `/sys` and `/slack` do not describe their ROOTS (`describe /sys` →
  `unsupported_verb`), so `cd` there fails before the gate is consulted; and slack has no "channel
  tree" node at all — its addressable nodes are `<#chan>/messages`, `replies`, `reactions`, `dms`,
  `files`, `users`, every one a leaf (`files` is already navigable as a `BlobNamespace`).
  **Context**: Making `cd /sys` work is new driver SURFACE (a root node + its schema), not a flag —
  which is why the Quality Gate names only the four reachable interiors. A follow-up ticket should
  decide whether `/sys` and `/slack` roots become describable catalog nodes.

- **Insight**: `cd` into a blob FILE still succeeds (`cd README.md` moves the cwd) and this slice
  cannot fix it. `Driver::describe` is pure — no I/O — so `LocalFsDriver` cannot stat a path to tell
  a file from a directory and answers `BlobNamespace` for every path, file or not.
  **Context**: Pre-existing, unchanged here, and structurally unfixable at describe time without
  breaking the purity invariant. The honest options are a describe that reports per-path kind from a
  cached listing, or a gate that tolerates it. Recorded in blueprint §9 as a conformance follow-up.

- **Insight**: `NodeDesc` is a serialized wire DTO — adding a field breaks JSON snapshot goldens in
  crates that never touched this feature (`qfs-driver`'s `describe_json_snapshot_is_stable` and
  `qfs-driver-slack`'s per-archetype snapshot both needed re-blessing).
  **Context**: `#[non_exhaustive]` protects the constructor, not the wire shape. Any future
  `NodeDesc` field pays the same tax; the two snapshots are the canary and should be expected to go
  red together.
