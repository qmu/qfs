---
created_at: 2026-08-17T10:27:23+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: 20260817102723-survey-the-documentation-surface-and-map-it-against-what-ships-today.md
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff: 
---

# Document the repository as it stands: both packages, the gates, the anti-drift generators and the release path

## Overview

PROPOSED. The sibling ticket documents the system; this one documents the **repository** —
what a contributor arriving today has to know before touching anything. That knowledge exists
only in `CLAUDE.md` (agent guidance, not reader documentation) and in the workflows: the
monorepo holds two projects, `packages/qfs/` (the Rust binary) and `packages/qfs-viewer/` (a
TypeScript markdown knowledge browser imported as a snapshot), each with its own gate runner;
three anti-drift generators decide which files may never be hand-edited; and the deliverable
is a published GitHub Release cut from a tag, with no server deployment.

`packages/qfs-viewer/` is the sharpest gap: it reaches the docs site only through two blueprint
sections, so a reader of the documentation would not learn that half the monorepo exists.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `CLAUDE.md` and `packages/qfs-viewer/CLAUDE.md` — where the gates, the generators and the
  version rule are currently written down; the source for this page, and the thing it must not
  simply duplicate.
- `packages/qfs-viewer/` — its `README.md`, `scripts/check-all.sh` and `packages/` layout: the
  package whose current state the documentation does not carry.
- `packages/qfs/xtask/` — `gen-docs`, `gen-skills`, `check-migrations`: the three anti-drift
  checks and what each one holds.
- `.github/workflows/release.yml`, `install.sh`, `packages/qfs/crates/qfs/Cargo.toml` — the
  release path from a patch bump to the four native tarballs a user installs.
- `docker-compose.yml`, `Dockerfile.docs`, `containers/`, `deploy/` — how the docs site and the
  containers are actually run.
- `docs/.vitepress/` — the new page has to be reachable from the site's navigation.

## Implementation Steps

1. Describe the monorepo as it is: the two packages, what each is, which is the product, and how
   the qfs-viewer snapshot import relates to its upstream repository.
2. Document `packages/qfs-viewer/`'s current state to the depth the survey ticket's map found
   missing — what it is, how it runs, what its gate runner covers.
3. Document the verification surface: each gate command, what it proves, and which failures it
   is the only thing that would catch.
4. Document the three anti-drift generators, each with the files it owns and the reason those
   files must never be hand-edited.
5. Document the version and release path end to end: the per-PR patch bump, the tag, the release
   workflow's four tarballs, `install.sh`, and the parked Workers artifact — parked stated as
   parked, never as shipped.
6. Write it as a page on the docs site, added to the navigation, dated and carrying the commit it
   was verified against.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Both packages are described, and qfs-viewer's section is written from that package's own tree
  rather than from the blueprint sections about it.
- Every gate command in the page runs as written from the directory the page names.
- Every generated-file claim names its generator, and every one of those generators exists.
- The release path is described from the workflow file, and anything parked is labelled parked.
- The page carries the commit and date it was verified against and is reachable from the site's
  navigation.

**Verification method** — the commands/tests/probes that prove them:

- Run each gate command the page lists, from the directory it names, and record the exit codes.
- Resolve every file path the page cites at the recorded commit.
- `docker compose up docs` and open the page — it renders and its links resolve.

**Gate** — what must pass before approval:

- `cd packages/qfs && cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check`, `gen-skills --check` all exit 0.
- `cd packages/qfs-viewer && ./scripts/check-all.sh` exits 0.

## Considerations

- Duplicating `CLAUDE.md` into the docs site creates a second copy that will drift from the first.
  The page should be the reader-facing account and `CLAUDE.md` the agent-facing one, with the
  overlap stated rather than silently doubled — how far that goes is worth deciding in this
  ticket's own discovery.
- `check-migrations` needs release tags to run, so a clone without tags cannot verify that gate;
  the page must say so rather than list a command that fails for a reader.
