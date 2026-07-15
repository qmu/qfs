---
created_at: 2026-07-11T12:15:33+09:00
author: a@qmu.jp
type: housekeeping
layer: [Config, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Dependency reduction: assess the current posture and execute the realistic levers

## Overview

Answer the mission's question — "check how far we tried, and how much can we do from here" — on
top of the existing baseline: the dependency-posture overhaul (09739aa) already produced
blueprint §11's per-dependency decision log (~29 direct deps) and identified the real levers as
**feature-trimming the heavy transitive roots** (reqwest TLS/blocking features, the three DB
drivers, tokio, tracing-subscriber), not shaving convenience crates (removable-today ≈ 0 for
thiserror/base64/bytes/rpassword; async-trait is a monitored exit). This ticket (a) re-measures
the tree as of v0.0.42 (the transform epic and new drivers may have widened it), (b) executes the
feature-trim levers that are actually available now, and (c) updates §11 with the new numbers and
a dated "how much further is realistic" ruling — adopt-with-plan or defer-with-reasoning per lever.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/vendor-neutrality.md` — the governing policy: implement by default, log Reason/Assessment/Monitoring/Exit per dependency in the single §11 log
- `workaholic:operation` / `policies/ci-cd.md` — the purity dep-closure test and wasm build are the mechanical guards any trim must keep green

## Key Files

- `docs/blueprint.md` - §11 dependency posture decision log (the artifact to update)
- `packages/qfs/Cargo.toml` - workspace root; tokio confinement policy stated inline
- `packages/qfs/crates/driver-http/Cargo.toml` - reqwest (rustls, no default features) confinement leaf
- `packages/qfs/crates/qfs/Cargo.toml` - the terminal binary where all feature-gated coupling composes
- `packages/qfs/crates/host/Cargo.toml` - mutually-exclusive host features (clippy NOT --all-features)

## Related History

- [20260706194536-dependency-posture-overhaul.md](.workaholic/tickets/archive/work-20260706-204906/20260706194536-dependency-posture-overhaul.md) - the audit baseline; extend, don't restart

## Implementation Steps

1. Measure: `cargo tree` / `cargo metadata` snapshot of direct + transitive counts per crate as of HEAD; diff against §11's recorded numbers; attribute growth (transform epic? cf driver?).
2. Execute available trims: re-check reqwest/tokio/tracing-subscriber/DB-driver feature flags against what the code actually uses; drop unused features; verify the purity dep-closure test, wasm32 build, and the full gate suite stay green.
3. Rule the remaining levers: for each §11 lever not executed, write the dated ruling (what it saves, what blocks it, adopt-with-plan or defer-with-reasoning) — including the async-trait exit condition status.
4. Update blueprint §11 with the new snapshot and rulings; regenerate docs if any generated page reflects dependency claims.

## Quality Gate

**Acceptance criteria**

- Blueprint §11 carries a dated v0.0.42-era snapshot (direct/transitive counts, growth attribution) and a per-lever ruling.
- Every executed trim keeps `cargo build/test/clippy/fmt`, the purity dep-closure test, and the wasm32 build green.
- Binary size and dependency counts before/after are recorded as numbers, not adjectives.

**Verification method**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, wasm32 `--no-default-features` build, and `cargo tree` diffs quoted in the ticket's final report.

**Gate**

- All gates green + §11 updated is the /drive approval gate; no live/owner round needed (local-only work).

## Considerations

- Mutually exclusive qfs-host features mean clippy must NOT run --all-features (CLAUDE.md) — trims must be validated per feature set (`packages/qfs/crates/host/Cargo.toml`)
- Growth from the transform providers ticket (if landed first) belongs in the snapshot — sequence awareness, not a dependency edge

## Resolution (2026-07-14, v0.0.62 → v0.0.63)

Re-measured the tree at HEAD and recorded a dated v0.0.62-era snapshot + per-lever ruling in
blueprint §11 ("Re-measurement (2026-07-14, v0.0.62)"). **No trim was executed — none was available
without dropping a used capability**, the honest defer-with-reasoning outcome the mission's question
asks for.

**Numbers (reproducible method: `cargo tree -p qfs --edges normal`, host target, default features,
deduped by `(name, version)`):**
- Workspace members: **50** (unchanged from v0.0.54).
- Direct third-party deps: **31** incl. `chumsky` (throwaway `parser-spike` only) → **30 shipped**,
  identical to v0.0.54 crate-for-crate.
- Tree size: **binary 356 crates, full-workspace 363** (re-baselined; the v0.0.54 `334/466` pair
  used an unstated, non-reproducible method — the shipped direct-dep count is the comparable figure
  and is flat).

**Growth attribution:** zero new third-party crates and zero new workspace crates since v0.0.54 —
every mission delivery in the window (file-handling bytes rounds, Gmail→Drive transfer, `att<N>`
read, transform chain/switch/PDF, `AUTH ACCOUNT` declared drivers, sweeper/scheduling fixes) reused
already-present crates and added no dependency edge.

**Lever rulings (dated 2026-07-14):**
- `reqwest` (no-default + rustls), `mysql` (`minimal`), `postgres` (explicit OID set),
  `rusqlite` (`bundled`), `tokio` (per-crate feature-scoped) — **already executed** (re-verified pinned).
- `tracing-subscriber` `env-filter` — **defer-with-reasoning**: still a used `RUST_LOG` capability
  (`EnvFilter::try_from_default_env`); the 111:3 flat:span ratio makes a minimal-formatter rewrite
  *available* but it would re-implement env filtering for no crate removed.
- `async-trait` (61 `dyn …Driver` sites) — **monitored exit** (adopt native `dyn` async-trait
  dispatch when stable/ergonomic); unchanged.
- Transitive duplicate versions (`base64`/`rand`/`getrandom`/`hashbrown`) — **upstream-owned**, not
  a lever this workspace controls.

**Gates:** no Rust changed (blueprint is hand-authored, not a gen-docs input); the full CLAUDE.md
gate suite was run green at branch finalize. Closes mission acceptance line
"Dependency reduction … adopt-with-plan or defer-with-reasoning ruling".
