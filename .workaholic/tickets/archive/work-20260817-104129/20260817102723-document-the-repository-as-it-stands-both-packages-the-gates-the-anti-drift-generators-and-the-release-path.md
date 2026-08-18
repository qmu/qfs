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

## Final Report

Development completed as planned. `docs/guide/repository.md` describes the repository at commit
`52b0410`, in the site navigation under "The project as built", beside the architecture page and
the documentation map.

### What was verified

**Every gate command the page lists was run as written, from the directory the page names.**

| Command | Directory | Exit |
| --- | --- | --- |
| `cargo build --workspace` | `packages/qfs` | 0 |
| `cargo test --workspace` | `packages/qfs` | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | `packages/qfs` | 0 |
| `cargo fmt --all --check` | `packages/qfs` | 0 |
| `cargo run -p xtask -- gen-docs --check` | `packages/qfs` | 0 |
| `cargo run -p xtask -- gen-skills --check` | `packages/qfs` | 0 |
| `cargo run -p xtask -- check-migrations` | `packages/qfs` | 0 |
| `./scripts/npm-install.sh` | `packages/qfs-viewer` | 0 |
| `./scripts/check-all.sh` | `packages/qfs-viewer` | **1** — see below |

`check-migrations` did resolve a baseline here: the clone carries 22 tags, `v0.0.71` … `v0.0.108`.
The page states the no-tags case (returns clean rather than failing) as the caveat it is.

Every file path the page cites was resolved at this commit — 60 backticked paths, checked against
the tree with each section's directory context. One was wrong on the first pass and is fixed: the
marketplace manifest is the repository-root `.claude-plugin/marketplace.json`, not
`plugins/qfs/.claude-plugin/marketplace.json` as `CLAUDE.md`'s phrasing suggests. All four plugin
version fields read `0.20.0`, and the second manifest (`.agents/plugins/marketplace.json`) carries
no version, which the page now says.

Every generated-file claim names its generator and each generator exists: `gen-docs`, `gen-skills`
and `check-migrations` are the three `xtask` subcommands (the fourth, `dist`, is the release
builder, not an anti-drift check). The release path is described from
`.github/workflows/release.yml` and `xtask`'s `NATIVE_TARGETS`; the Workers wasm artifact is
labelled parked, matching `WASM_TARGET`'s own comment and `docs/server.md`'s deployment table.

The page renders: driven in Chromium against `vitepress dev` on port 4101 — `/guide/repository` →
200, H1 correct, 14 sections, 35 table rows, the sidebar entry present, and both internal links
(`/guide/architecture`, `/documentation-map`) → 200. The only network failure is the VitePress
theme's GitHub icon from `api.iconify.design`, blocked by this environment's proxy and identical on
an untouched page.

### The one gate that does not pass, and why

`./scripts/check-all.sh` exits **1**. Both structural gates, the dist build, the node leg of the
npx smoke and both unit suites pass; the **bun** leg of the smoke fails inside a published
dependency — `plgg-md@0.0.3`'s dist carries a regex bun 1.3.11 rejects
(`SyntaxError: Invalid regular expression: range out of order in character class`). `plgg-md@0.0.3`
is the newest version on the registry, so the declared `^0.0.3` already resolves to it and no bump
can fix it; the fix is upstream in the private `qmu/plgg` repository.

This is a named external blocker, not a failure of this change: no TypeScript was touched, and the
same command fails identically on an unmodified tree. It is also invisible to CI, whose
`viewer-check-all` job installs Node only, so the smoke skips bun and deno there — a gate whose
meaning depends on which runtimes the machine happens to carry. Filed with the raw output as
`20260817111530-the-qfs-viewer-gate-cannot-pass-where-bun-is-installed.md`.

### Discovered Insights

- **Insight**: the multi-line inline-code span that breaks the production docs build is not
  exotic — this page hit it three times while being written (`docker compose up docs`,
  `git tag -a … && git push`, `cargo run -p xtask -- dist --target <triple>`, each wrapped across
  a line break). A span split over two lines loses its code formatting, and any `<word>` inside it
  reaches the Vue compiler as an unclosed element, taking the whole build down.
  **Context**: this is the same defect already open against `docs/blueprint.md`
  (`20260817110309-…`), so the fix there should be a repository-wide scan plus a CI docs build,
  not a one-page correction — writing prose in this repository will keep re-introducing it until a
  gate catches it.

- **Insight**: both of qfs-viewer's structural gates **self-test their own red/green logic** before
  enforcing anything, on every run. A gate never proven to fail is not a gate, and this repository
  implements that literally rather than as advice.
  **Context**: the pattern is worth copying to the Rust side, where `gen-skills --check` currently
  has neither a self-test nor a CI step.

- **Insight**: the npx smoke is the only check that exercises the packed artifact. Every unit suite
  runs TypeScript source, so the bin, the `files` list and the launcher could all be broken while
  everything else stayed green — and Node 24 refuses to strip types under `node_modules`, which is
  a failure mode that exists nowhere else in the gate.
  **Context**: when the bun leg above is settled, the smoke must keep running against the packed
  tarball; degrading it to a source-level check would silently remove the only proof of the
  product's headline promise.

- **Insight**: the anti-drift family and the release guard are asymmetric in enforcement.
  `gen-docs` drift fails `cargo test`; `gen-skills --check` and `check-migrations` are run by
  people, and `xtask dist` additionally refuses to run at all without `QFS_DIST_ALLOW=1` because a
  release build wedges a constrained disk.
  **Context**: "run the gates" means something different depending on which gate — the page now
  says which is which, and it is the reason a stale skill cache can ship while a stale reference
  page cannot.
