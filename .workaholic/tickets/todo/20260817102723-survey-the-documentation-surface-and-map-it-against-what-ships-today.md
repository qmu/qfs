---
created_at: 2026-08-17T10:27:23+00:00
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
