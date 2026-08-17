---
created_at: 2026-08-17T10:27:23+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff: 
---

# Survey the documentation surface and map it against what ships today

## Overview

PROPOSED. The mission's other two tickets write documentation; this one decides **what**
they must cover, so it runs first. The repository carries 35 markdown pages under `docs/`
plus two `CLAUDE.md` files and two package READMEs, and they are not one kind of thing:
`docs/{language,drivers,server}.md` are rendered from the binary by `xtask gen-docs`,
`docs/blueprint.md` is intent carrying per-section status, and the guide and cookbook are
hand-written usage. A reader cannot currently tell which is which, nor what none of them
covers.

The output is a **map**, not prose about documentation: one row per page — what it claims,
whether it is generated or hand-written, and what current state it does not reach. The gaps
this proposal already sees (there is no page describing the built system's shape, and
`packages/qfs-viewer/` reaches the docs site only through blueprint §14b/§14c) are
hypotheses for the survey to confirm or correct, not its conclusions.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `docs/` — the 35 pages under survey, including `.vitepress/` for what the site actually
  publishes (a page not in the sidebar is a different kind of gap from a missing page).
- `docs/blueprint.md` — the living design document; its `implemented`/`blueprint`/`parked`
  markers are the existing separation of fact from intent, and the survey records how far it
  is trusted.
- `packages/qfs/xtask/` — the generators (`gen-docs`, `gen-skills`), which decide which pages
  must never be hand-edited.
- `CLAUDE.md`, `packages/qfs-viewer/CLAUDE.md`, `README.md`, `packages/qfs-viewer/README.md` —
  documentation that is not on the docs site but is read first by contributors.

## Implementation Steps

1. Enumerate every documentation file in the repository (root `docs/`, both packages' READMEs
   and `CLAUDE.md`s, `plugins/qfs/skills/*/SKILL.md`), and mark each **generated** or
   **hand-written** by finding the writer — an xtask subcommand, or nothing.
2. For each page, read it and record what it claims to cover and its last substantive update
   from git history.
3. Establish what ships today independently of the docs: the workspace's crates, the binary's
   subcommands, the driver catalog the describe registry carries, the server surfaces, and the
   qfs-viewer package's entry points. Read them from the source and the binary, not from prose.
4. Diff the two: name each area of the shipped system no page covers, and each page that
   describes something the code no longer does.
5. Write the map to `docs/` (location decided in the ticket, see Open Decisions) with the date
   and the commit it was taken against.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Every documentation file in the repository appears in the map exactly once, marked generated
  or hand-written, with the writer named for each generated one.
- Every gap is stated as a specific uncovered area, never as "needs more detail".
- The map names the commit and date it was taken against.

**Verification method** — the commands/tests/probes that prove them:

- A file listing of the documentation surface compared against the map's rows — no file
  missing, none invented.
- Each "generated" claim checked by running the named generator with `--check`.

**Gate** — what must pass before approval:

- `cd packages/qfs && cargo run -p xtask -- gen-docs --check` and `gen-skills --check` exit 0
  (the survey must not have hand-edited a generated page).

## Considerations

- The survey must not become the documentation. Its product is the map; writing the missing
  pages is the other two tickets' work.
- Reading what ships from the docs would make the diff vacuous — the whole point is that the
  two sides are established independently.

## Open Decisions

- **Where the map lives, and whether it survives the mission.** It could be a durable page on
  the docs site (a reader-facing "what is documented where"), a contributor-facing file
  outside the site, or a working artifact that the mission's later tickets consume and that is
  then deleted. The three differ in what has to be maintained afterwards, and this proposal
  has no basis for choosing. The driving session resolves it explicitly and records the
  resolution in its Final Report.

## Final Report

Development completed as planned. The map is `docs/documentation-map.md`, dated 2026-08-17 and
pinned to commit `52b0410` / binary `qfs 0.0.108`, added to the VitePress sidebar under a new
"The project as built" section.

### Open Decision resolved

**Where the map lives, and whether it survives the mission** — resolved as a **durable page on the
docs site** (`docs/documentation-map.md`, in the navigation), not a contributor-facing file outside
the site and not a working artifact to delete.

Reasoning: the mission's `## Experience` asks that "a reader can tell of any page whether it is
generated from the binary or hand-written, and what it does not cover". That is this map, stated as
a *reader-facing* requirement — so a file outside the published site could not satisfy it, and a
working artifact deleted at the end would take the evidence the other two tickets were written
against with it. The maintenance cost the fork was really about is bounded the same way the two
sibling pages bound theirs: the page carries the commit and date it was taken against and says
plainly that nothing checks it, so a reader can tell how old its claims are instead of trusting them
indefinitely.

### Discovered Insights

- **Insight**: the repository has exactly three generated-file writers, and only one of them is
  defended automatically. `gen-docs` drift is caught by a unit test inside `qfs::docs`
  (`docs_drift_golden`), so `cargo test --workspace` and therefore CI enforce it; `gen-skills
  --check` and `check-migrations` have no test and no CI step — `.github/workflows/ci.yml` never
  invokes `xtask` at all.
  **Context**: any future anti-drift generator must ship with a unit test in a workspace crate, not
  just an `xtask` subcommand, or it is enforced only by whoever remembers to type it.

- **Insight**: `docs/drivers.md` is generated but structurally incomplete, and for a reason worth
  knowing. It renders the *compiled* cred-free describe registry, so `/sql`, `/git` and `/cf` are
  absent because their describe needs a registered connection catalog / repo / D1 catalog first (a
  registration requirement, not a credential one — `crates/qfs/src/describe.rs` documents the
  fallback), and every declared driver is absent because a declaration is operator-installed data.
  **Context**: "the driver catalog is generated from the binary, so it always matches" is true and
  still leaves five shipped mounts undocumented; a reader needs the reason, not just the table.

- **Insight**: the two packages are coupled in one direction only, and the coupling is recorded
  solely inside the smaller one. `packages/qfs-viewer` locates a `qfs` binary rather than bundling
  it and serves its corpus from qfs's markdown collection path (ADRs 0008 and 0009), while nothing
  under `docs/` mentions the package except two blueprint sections.
  **Context**: the qfs side of a real integration seam is undocumented, which is why the mission's
  third ticket has to read that package's own tree rather than the blueprint sections about it.

- **Insight**: `packages/qfs/ARCHITECTURE.md` is not merely out of date, it is out of date in a way
  that misleads structurally — a 20-crate map of a 48-crate workspace, predating the whole
  `qfs-exec` integration layer that now sits between `qfs-cmd` and the spine.
  **Context**: a contributor reading it would place the read path in `qfs-core` and miss that
  `qfs-exec` owns the end-to-end SELECT executor and the one-shot orchestration.
