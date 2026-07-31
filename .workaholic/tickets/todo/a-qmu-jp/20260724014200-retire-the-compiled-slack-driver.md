---
created_at: 2026-07-24T01:42:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Removed
depends_on: [20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md, 20260724014100-slack-call-maps-effect-equivalent.md]
mission: a-declared-write-resolves-a-name-the-way-a-query-does
---

# Retire the compiled slack driver

## Overview

The twin-and-retire ratchet's deletion step, gated on BOTH equivalence tickets green. Execute the
four shared retirement steps exactly as blueprint §13.3 records them:

1. **Delete the compiled driver crate** (`crates/driver-slack`) and its registration in
   `qfs::describe::compiled_describe_registry`.
2. **Regenerate the reference docs** — `cargo run -p xtask -- gen-docs` (the compiled `/slack`
   entry drops out of `docs/drivers.md`) and `cargo run -p xtask -- gen-skills` (any cookbook
   recipe naming the compiled path re-renders against the declared surface). Never hand-edit the
   generated files.
3. **Bump the plugin version MINOR** in all four fields (`plugins/qfs/.claude-plugin/plugin.json`,
   `.codex-plugin/plugin.json`, both `version` fields in `.claude-plugin/marketplace.json`) — a
   taught-surface break, so installed skill caches stop teaching the retired compiled path.
4. **Bump the binary patch** (`packages/qfs/crates/qfs/Cargo.toml`) per the every-shipped-PR rule
   (normally the /report step; keep the two bumps in the same PR).

This is a sanctioned hard break (qfs is experimental; no deprecation period, no compat shim). The
precedent is the /markdown retirement (v0.0.87, plugin 0.15.0).

## Policies

- Blueprint §13 twin-and-retire ratchet — deletion only after the equivalence gate is green; the
  gate stays in the tree as the regression proof of the declared twin.
- CLAUDE.md plugin re-versioning rule — minor bump, all four fields, same PR.
- experimental-no-backward-compat — no migration path for `CONNECT ... TO slack` compiled-era
  declarations beyond what the twin's install story already covers; the release note states the
  break plainly.

## Quality Gate

1. `crates/driver-slack` and its registry entry are gone; the workspace builds with no dangling
   references; `docs/drivers.md` no longer lists the compiled slack driver (gen-docs --check
   green after regeneration).
2. `gen-skills --check` green; no SKILL.md teaches the retired compiled path
   (grep proves it).
3. All four plugin version fields agree at the new MINOR version.
4. The row/effect-equivalence tests from the two prerequisite tickets still run green against the
   declared twin alone (they are the twin's regression suite now).
5. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check`, `cargo run -p xtask -- gen-skills --check`.

## Considerations

- The equivalence fixtures must survive the deletion (re-point the compiled side of the harness
  at recorded expected rows, as the /markdown retirement did with its oracle).
- Update blueprint §13.3's slack row status in the same commit so the playbook stays true.

## Not started — the ratchet's own gate is not green (run 20260725-101714)

The overnight `/monitor` drive reached this ticket and did NOT implement it. The reason is this
ticket's first line: it is *"the twin-and-retire ratchet's deletion step, gated on BOTH equivalence
tickets green."* Only one is.

- `20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md` — **done** (commit `cf75d76`):
  `slack_driver.qfs` is committed and every declared read is proven row-equivalent to `driver-slack`
  on shared hermetic fixtures, with the G2 pushdown parameters proven at the wire request.
- `20260724014100-slack-call-maps-effect-equivalent.md` — **still open** (partial, commit `362c499`).
  The G5 typed signatures landed and match the compiled registry exactly, but the five CALLs' WIRE
  effect equivalence is unproven and the channel-name→id resolution parity cannot be reproduced until
  blueprint §13.1 **G4** (per-row fan-out, `FOLLOW … INTO`) is implemented.

Deleting `driver-slack` now would remove the very oracle the outstanding equivalence proof compares
against — the ratchet run backwards. Blueprint §13.3's slack row states the bar as *"declared reads
row-equivalent … **AND** the 5 CALL maps + post map effect-equivalent"*; half a bar is not a bar.

**To unblock:** finish `20260724014100` (G4 fan-out → declared CALL dispatch → the five wire-level
equivalence tests), then run this ticket unchanged. Nothing in this ticket's own steps needs revising.

## Still not started (run 20260726-184527) — the gate moved closer but is not green

The overnight `/monitor` drive shipped blueprint §13.1 G4 (`365d521`) and closed the two remaining
sub-items of `20260724014100` that G4 could close — but **not QG2**, which turned out to need a
second ruled primitive (a declared reverse lookup on the write path, minted as
`20260726190000-declared-reverse-lookup-for-write-path-name-resolution.md`). Since the last run:

- `20260724014100` QG1 and QG3 remain closed (`73fa5de`); QG2 is still open, now with the precise
  structural reason recorded on both it and the G4 ticket rather than a pointer to unbuilt work.

This ticket's own first line remains the gate — *"gated on BOTH equivalence tickets green"* — and it
is not. `driver-slack` is still the oracle the outstanding proof compares against; deleting it now
destroys the evidence. Nothing in the four retirement steps needs revising.
