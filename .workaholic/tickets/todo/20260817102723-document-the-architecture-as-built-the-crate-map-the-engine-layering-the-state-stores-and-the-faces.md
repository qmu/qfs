---
created_at: 2026-08-17T10:27:23+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: 20260817102723-survey-the-documentation-surface-and-map-it-against-what-ships-today.md
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff: 
---

# Document the architecture as built: the crate map, the engine layering, the state stores and the faces

## Overview

PROPOSED. `packages/qfs/crates/` holds 43 crates and no page says what they are or how a
statement travels through them. `docs/blueprint.md` gives the intended design section by
section, `docs/guide/design-snapshot.md` gives the operating model a user sees, and the
generated references give the exact grammar, driver catalog and binding tables — but a
contributor or agent asking "where does a query get planned, and which crate owns pushdown"
has to read the workspace.

This ticket adds the missing page: the system as **built**, read from the source, dated, and
carrying the commit it was verified against. It is description, not design — where the source
and the blueprint disagree, the page records what the code does and names the disagreement
rather than resolving it.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `packages/qfs/crates/` — every crate is in scope for the map; `Cargo.toml` dependency edges
  are the evidence for the layering, not the crate names.
- `packages/qfs/crates/{parser,lang,plan,pushdown,exec,engine,runtime}` — the query path a
  statement travels; the page's spine.
- `packages/qfs/crates/{driver,driver-type,driver-*}` — the driver contract and the compiled
  drivers that implement it, alongside the declared drivers the DSL carries.
- `packages/qfs/crates/{store,secrets,session,identity,provision}` — the state stores
  `design-snapshot.md` names as Project DB and System DB, and the vault beside them.
- `packages/qfs/crates/{cmd,qfs,http,server,mcp,host}` — the faces: CLI, shell, HTTP server, MCP.
- `docs/guide/design-snapshot.md` — the existing current-state page the new one must sit beside
  without duplicating; the boundary between them is decided here.
- `docs/.vitepress/` — the new page has to be reachable from the site's navigation.

## Implementation Steps

1. Build the crate map from `Cargo.toml` dependency edges: for each crate, one line saying what
   it owns, plus the edges that place it in the layering. Derive it mechanically; do not infer
   from names.
2. Trace one read statement and one write statement end to end through the crates, naming the
   entry point of each stage. This is the page's spine and it is what makes the map usable.
3. Record the state stores as they exist: which store each schema lives in, where the files sit
   on disk, and what the vault holds versus what the stores hold.
4. Record the faces the binary actually serves, each with the entry point that starts it.
5. Write `docs/guide/architecture.md` from the above, add it to the VitePress sidebar, and state
   at the top the commit and date it was read against.
6. Where the source contradicts `docs/blueprint.md`, name the contradiction in the page and file
   it back to the mission rather than editing the blueprint here.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Every crate under `packages/qfs/crates/` appears in the map exactly once.
- The read trace and the write trace each name a real entry point per stage, and each named
  symbol exists in the source at the recorded commit.
- The page carries the commit and date it was verified against, and is reachable from the docs
  site's navigation.
- No claim in the page restates a blueprint section marked `blueprint` or `parked` as current
  fact.

**Verification method** — the commands/tests/probes that prove them:

- Compare the map's crate list against the workspace members listed by `cargo metadata`.
- Resolve every symbol and path the traces name in the source at the recorded commit.
- `docker compose up docs` and open `/guide/architecture` — the page renders and its links resolve.

**Gate** — what must pass before approval:

- `cd packages/qfs && cargo run -p xtask -- gen-docs --check` exits 0 (no generated page edited).
- The docs site builds, and the mission's `documentation` gate route `/guide/architecture` serves.

## Considerations

- The risk is a page that is true on the day it lands and quietly wrong a month later. Dating it
  and naming the commit is the minimum; whether the crate list should be checked mechanically
  the way `gen-docs` checks the references is a question for the mission, not this ticket.
- The overlap with `docs/guide/design-snapshot.md` is real and must be resolved by drawing the
  boundary explicitly in both pages, not by writing a second operating model.
