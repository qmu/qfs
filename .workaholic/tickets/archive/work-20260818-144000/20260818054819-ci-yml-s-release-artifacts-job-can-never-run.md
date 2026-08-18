---
created_at: 2026-08-18T05:48:19+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260818-144000
---

# `ci.yml`'s `release-artifacts` job can never run

## Overview

Observed while implementing
`20260817164716-a-tag-can-publish-reference-docs-that-drifted-from-the-binary.md`, whose whole
argument turned on establishing what `ci.yml` does and does not run on a tag push.

`.github/workflows/ci.yml` declares:

```yaml
on:
  push:
    branches: ["**"]
  pull_request:
```

and its last job is:

```yaml
  release-artifacts:
    name: release artifacts (musl + wasm + wrangler + checksums)
    if: startsWith(github.ref, 'refs/tags/')
```

The `branches:` filter selects **branch** pushes only — a tag push needs a `tags:` filter, which
this workflow does not have. So `github.ref` is `refs/heads/<branch>` for every push that triggers
this workflow and `refs/pull/<n>/merge` for every `pull_request` event. Neither starts with
`refs/tags/`, so the `if:` is unsatisfiable and the job has never executed and cannot execute.

It is not merely dormant, it is **superseded**: `release.yml`'s `build` matrix is what actually
produces the release tarballs on a `v*` tag, on per-OS runners that can link each target, and it
is what `install.sh` consumes. `release-artifacts` still calls `deploy/release.sh` and uploads to
an artifact named `qfs-release` that nothing downloads.

The cost of leaving it is not CI minutes — it burns none — but that `ci.yml` reads as though it
covers the release path when it does not, which is precisely the misreading that produced the
ticket this one was minted from.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` / `policies/ci-cd.md` — what the delivery path is and where each gate
  in it lives

## Key Files

- `.github/workflows/ci.yml` — the `on:` block and the `release-artifacts` job at the end.
- `.github/workflows/release.yml` — `build` (four native tarballs via `xtask dist`), `release`,
  `docs-drift`, `docs-deploy-production`: the pipeline that a tag actually runs.
- `packages/qfs/deploy/release.sh` — the script `release-artifacts` calls. Decide whether anything
  still needs it once the job is gone; `xtask dist` is what `release.yml` uses.
- `docs/guide/repository.md` — its verification-surface section enumerates CI's jobs and would
  need to stop listing this one.

## Related History

Minted from `20260817164716-a-tag-can-publish-reference-docs-that-drifted-from-the-binary.md`,
which established that a tag push runs nothing from `ci.yml` and added `release.yml`'s `docs-drift`
job for that reason. This ticket is the other half of that finding — the part that was outside the
provoking ticket's scope and so was not fixed opportunistically.

## Implementation Steps

1. Confirm the job has never run: inspect the workflow-run history for `ci.yml` on a `v*` tag and
   for `release-artifacts` specifically. The reading above is from the YAML alone; confirm it
   against the actual run list before deleting anything.
2. Decide between the two live options and record the reasoning:
   - **Delete the job.** `release.yml` already produces every artifact a release needs, so the
     job is redundant as well as unreachable.
   - **Make it reachable** by adding `tags: ["v*"]` to `ci.yml`'s `on: push:`. This is almost
     certainly wrong — it would duplicate `release.yml`'s build on every tag — but it is the
     option under which the wasm/wrangler/checksums outputs, which `release.yml` does not
     produce, would come back. Establish whether anything consumes them first.
3. Implement the choice. If deleting, check whether `packages/qfs/deploy/release.sh` still has a
   caller, and either keep it as the documented by-hand path or retire it in the same change.
4. Update `docs/guide/repository.md` where it enumerates CI's jobs.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- No job remains in `.github/workflows/ci.yml` whose `if:` condition cannot be satisfied by any
  trigger the workflow declares.
- A `v*` tag still publishes the four native tarballs and their `.sha256` files to the GitHub
  Release, and `install.sh` still resolves them.
- `docs/guide/repository.md`'s list of CI jobs matches `ci.yml`.

**Verification method** — the commands/tests/probes that prove them:

- Parse both workflows and cross-check each job's `if:` against the declared triggers.
- On the next release tag, confirm the Release carries all four `qfs-<target>.tar.gz` assets and
  their checksums.
- `git grep -n "release-artifacts"` returns no stale references outside history.

**Gate** — what must pass before approval:

- The whole existing CI matrix stays green on an unmodified tree.
- `release.yml`'s four-tarball path is untouched by the change.

## Considerations

- Do not fold this into a change that also touches `release.yml`'s publish path. The value of this
  ticket is that it removes a misleading claim; a change that also moves real release machinery
  makes the diff hard to reason about at exactly the point where a mistake is expensive.
- The wasm artifact is parked (`release.yml` says so in as many words, and `docs/adr/0005` records
  why), so "we would lose the wasm output" is not an argument for keeping the job — that output
  does not exist to lose.

## Final Report

Development completed as planned. The job was deleted, and `packages/qfs/deploy/release.sh` — the
script it was the only caller of — was retired with it.

**Step 1, the confirmation the ticket demanded before deleting anything.** The YAML reading was
checked against the actual run list rather than trusted: `ci.yml` has **zero** workflow runs on
`v0.0.111` (the newest tag), on `v0.0.97`, or on `v0.0.71` (the oldest tag still present), while
`release.yml` has exactly one run on `v0.0.111`. The job never executed, as read.

**Step 2, the decision — delete, not `tags: ["v*"]`.** The three outputs only `release-artifacts`
produced are each unconsumed. The musl `--features host-daemon` binaries are not what ships:
`release.yml`'s `xtask dist` builds `-p qfs` with default features, and `install.sh` resolves
*those* tarballs. `qfs.wasm` is parked with no cdylib entrypoint, so `release.sh` wrote a
`.PARKED` note in its place rather than an artifact. The `wrangler.toml.template` was a copy of a
golden fixture that stays in the tree at `crates/host/fixtures/wrangler.golden.toml`. Making the
job reachable would therefore have duplicated `release.yml`'s build on every tag in exchange for
artifacts nothing downloads.

**Step 3, `deploy/release.sh` — retired, not kept as a by-hand path.** With the job gone it has no
caller anywhere in the tree, and its own header states that it "is RUN IN THE CI release job" — a
claim that would have become false in the same commit that removes the job, which is precisely the
misreading this ticket exists to remove. It is not usable as a by-hand path either: the static
musl cross-link is CI-only (t01/A2), which is what the script's own header says. `deploy/qfs.service`
stays — it is a live template, and `crates/cmd/tests/e2e_serve.rs` pins its `KillSignal=SIGTERM`
graceful-drain claim.

**Step 4, the docs.** `docs/guide/repository.md`'s enumeration of CI's jobs ("fmt, clippy (three
invocations), build + test, two cross-compiles, a wasm32 host-core build, the docs site production
build, and the viewer gate") already omitted `release-artifacts`, so deleting the job is what makes
that sentence true rather than requiring an edit to it. The `deploy/` row did need one — it
described the directory as "`release.sh` and the deployment scripts CI calls", and both halves of
that are now wrong.

A comment stands where the job was, so the next reader does not re-add it: this workflow's `on:`
block selects branch pushes and pull requests only, a tag-gated job here can never execute, and the
release pipeline is `release.yml`.

### Discovered Insights

- **Insight**: A tag-gated job in a workflow whose `on: push:` carries only a `branches:` filter is
  not dormant — it is unreachable, and GitHub reports nothing about it. There is no warning, no
  skipped-job entry, and no run to inspect; the job's absence from the run list is indistinguishable
  from the workflow simply not having been triggered. The only way to see it is to cross-check each
  `if:` against the declared trigger set, which is why this ticket's verification method is a
  parse-and-cross-check rather than a log inspection.
  **Context**: This repository's two workflows partition the trigger space exactly — `ci.yml` takes
  branch pushes and pull requests, `release.yml` takes `v*` tags — so any `refs/tags/` condition
  inside `ci.yml`, and any `refs/heads/` condition inside `release.yml`, is dead by construction.
  That partition is the thing to check a new conditional job against.

- **Insight**: `packages/qfs/deploy/release.sh` and `xtask dist` were two release-artifact builders
  that never met. The first built raw musl daemon binaries plus a parked wasm module; the second
  builds the four stripped, tarballed, checksummed native artifacts `install.sh` actually resolves.
  They coexisted for as long as they did because the one thing that would have forced a comparison
  — a CI run executing both — could not happen.
  **Context**: When retiring a build path here, the question that settles it is which artifact the
  installer resolves. `packages/qfs/install.sh` names `qfs-<target>.tar.gz` and verifies its
  `.sha256`; only `xtask dist` produces that shape.
