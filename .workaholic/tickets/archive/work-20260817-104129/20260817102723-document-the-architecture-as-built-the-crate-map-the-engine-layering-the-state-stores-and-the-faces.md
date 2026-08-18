---
created_at: 2026-08-17T10:27:23+00:00
status: done
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

## Final Report

Development completed as planned. `docs/guide/architecture.md` describes the system as built at
commit `52b0410` / binary `qfs 0.0.108`, and is reachable from the site navigation under "The
project as built".

### What was verified

- **Every crate appears exactly once.** The 48 rows were checked against
  `cargo metadata --no-deps` (50 workspace members: 48 under `crates/`, plus `xtask` and
  `spikes/parser-spike`, both named in the page's opening paragraph). Zero missing, zero doubled.
- **Every symbol the two traces name resolves in the source** at this commit — 43 checks over
  `run_oneshot`, `parse_statement`, `plan_query`, `partition_by_source`, `execute_read`,
  `ReadDriver::scan`, `apply_where`, the `CombineEngine::execute` impl on `MiniEvaluator`,
  `apply_codecs`, both renderers, `Evaluator::eval`/`with_stdlib`, `resolve_statement`,
  `check_expr`, `build_plan`, `plan_preview`, `qfs_plan::preview`, `apply_via`,
  `commit::apply_plan_rooted`, `Interpreter::commit`, `CapabilitySet`, `PlanApplierBridge`,
  `scan_targets`, `Plan::pure`, both migration constants, the three path resolvers, and every
  face's entry point.
- **The page renders and its links resolve.** Driven in Chromium against `vitepress dev` on the
  worktree's docs port (4101): `/guide/architecture` → 200, H1 correct, 14 sections, 84 table
  rows, the new sidebar section and link present, and all four internal links
  (`/blueprint`, `/guide/design-snapshot`, `/guide/cli`, `/documentation-map`) → 200. The only
  network failure is the VitePress theme's GitHub icon from `api.iconify.design`, blocked by this
  environment's proxy and identical on an untouched page.
- **`cargo run -p xtask -- gen-docs --check` exits 0** — no generated page was edited.

### Gate not fully met, and why

The ticket's gate also asks that "the docs site builds". **`npm run docs:build` fails**, and the
failure is pre-existing and unrelated to this page: `vitepress build` aborts on
`docs/blueprint.md`, where a multi-line inline-code span (`` `CREATE TABLE <path> OF `` … ``
`<name>` ``, source lines 266-267) is not parsed as code, so its `<path>`/`<name>` reach the Vue
compiler as unclosed elements. `blueprint.md` is untouched by this ticket. It went unnoticed
because the project serves the site with `vitepress dev`, which compiles pages on demand, and no
gate anywhere runs the production build.

Rather than fix it inside a commit about the architecture page, it is filed as
`20260817110309-the-docs-site-production-build-fails-on-blueprint-md.md`, which also carries the
missing CI docs job. The mission's own `documentation` gate — the dev server serving
`/guide/architecture` — is met, and that is what was driven.

### The blueprint disagreements, named not resolved

Per the ticket's step 6, three are recorded in the page's *Where this page and the blueprint
differ* section and nothing in `blueprint.md` was edited: §14 (console) is marked `blueprint`
while `crates/qfs/src/console.rs` implements its delivery contract and the dashboard is the served
face; §19 (agents) is marked `blueprint` while `qfs agent run` ships with a live policy gate under
the agent's own subject; and `packages/qfs/ARCHITECTURE.md` contradicts the workspace itself.

### Discovered Insights

- **Insight**: `qfs-exec` exists because of a topology constraint, and knowing that explains the
  whole read path. The read executor needs `pushdown` + `engine` + `core` and async scans, but
  `runtime`'s spine is pinned to `{plan, types, txn}` and `cmd` must stay logic-free — so the
  executor could live in neither, and a new integration crate above the spine was the only place
  left. It carries its **own** async `ReadDriver` seam rather than taking the runtime edge,
  because the runtime's write `ApplyDriver` returns affected counts and never rows.
  **Context**: a future read-side feature belongs in `qfs-exec`, not in `qfs-core`, and adding a
  `qfs-runtime` edge to `qfs-exec` would break the confinement guard for no gain.

- **Insight**: a pushed `WHERE` is re-applied locally after every scan
  (`qfs_engine::apply_where`, skipped only for facets declaring `honors_pushed_filter`). The
  pushdown is a narrowing *hint*, never a delegation of correctness, so a driver that ignores it
  over-returns and gets filtered rather than answering an unfiltered relation at exit 0.
  **Context**: this is the invariant that makes federation safe to extend — a new driver may
  implement pushdown badly and still cannot return wrong rows.

- **Insight**: `local_root` is a correctness parameter, not a convenience. The `/local` root the
  context planned and previewed under must be the root the commit applies under; the one-shot,
  job and server contexts root at `/` while an interactive session roots at its cwd on both
  faces, which is why `apply_plan_rooted` exists beside `apply_plan`.
  **Context**: any new launch context (a second server face, an agent runner) has to thread its
  own root through both preview and commit or it silently mis-targets writes.

- **Insight**: the binary is deliberately the only crate that may name a file path or depend on a
  concrete driver — the store openers, the describe registry, the shell, serve, MCP and the
  commit registry are all composition roots in `crates/qfs/src/`, injected downward into
  `qfs-cmd` through launcher traits.
  **Context**: "where do I wire a new driver in?" has exactly one answer, and putting it anywhere
  else trips `dep_direction.rs`.
