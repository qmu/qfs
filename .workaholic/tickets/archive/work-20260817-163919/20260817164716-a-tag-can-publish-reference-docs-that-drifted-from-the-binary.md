---
created_at: 2026-08-17T16:47:16+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [20260817142443-a-release-publishes-the-docs-site-to-qfs-qmu-co-jp.md]
mission: the-documentation-site-publishes-itself-staging-on-merge-production-on-release
merge_policy:
verification_handoff:
---

# A tag can publish reference docs that drifted from the binary

## Overview

Minted while implementing
`20260817142443-a-release-publishes-the-docs-site-to-qfs-qmu-co-jp.md`, whose Considerations
named the problem and correctly left it to a separate ticket.

`docs/language.md`, `docs/drivers.md` and `docs/server.md` are generated from the binary's own
registries by `cargo run -p xtask -- gen-docs`. Nothing in CI runs `gen-docs --check`: `CLAUDE.md`
says so in as many words ("anti-drift: committed docs must match the binary … which no CI job
invokes and which therefore hold only where a developer or the ship flow runs them"), and reading
`.github/workflows/ci.yml` confirms it — `fmt`, `clippy`, `build-test`, `cross`, `docs-build`,
`viewer-check-all`, `wasm32-host-core` and `release-artifacts`, none of which invoke `gen-docs`.

Until now the consequence of that gap was a stale page in a repository. As of the change that
provoked this ticket, a `v*` tag publishes the tagged commit's documentation to qfs.qmu.co.jp,
so the same gap now publishes reference pages that can contradict the binary `install.sh`
installs from the very same tag. That is the observed change in blast radius, and it is what
makes this a queue entry rather than a note.

The fix is not obviously "add the check to the docs publish job" — that is one of at least three
placements, each with a different failure mode, which is why this is a ticket to decide and
implement rather than a one-line patch.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` / `policies/ci-cd.md` — where a release gate belongs in the delivery
  path, and what a failed gate does to a release already in flight

## Key Files

- `.github/workflows/ci.yml` — the eight jobs that exist today; none runs `gen-docs --check`.
- `.github/workflows/release.yml` — `build` → `release` → `docs-deploy-production`; the tag path
  a drifted page would travel.
- `packages/qfs/xtask/src/` — the `gen-docs` command and its `--check` mode.
- `docs/language.md`, `docs/drivers.md`, `docs/server.md` — the generated pages at risk.
- `CLAUDE.md` — states the gap; must stop stating it once this lands.
- `.workaholic/deployments/docs-site.md` — the deployment record; a gate added to the publish
  path belongs in its Procedure.

## Implementation Steps

1. Reproduce the gap: hand-edit a generated page in a scratch checkout, confirm the full CI
   matrix stays green, and confirm `cargo run -p xtask -- gen-docs --check` catches it. This
   establishes that the check works and that nothing else is already covering it.
2. Decide the placement and record the reasoning. The candidates, with what each costs:
   - a new `gen-docs --check` job in `ci.yml` — catches drift on the branch, before merge, at
     the price of a Rust build in a job that has none today;
   - a step inside `release.yml`'s `docs-deploy-production` job — catches it at the last
     moment, but the tag and the GitHub Release already exist by then, so the only available
     outcome is a red job and an unpublished docs site;
   - a gate on the `release` job itself — refuses to publish the Release at all, which is the
     strongest reading but also stops a binary release for a documentation defect.
3. Implement the chosen placement, including what a failure looks like to the person who
   pushed the tag.
4. Correct `CLAUDE.md` where it records that no CI job invokes `gen-docs --check`, and the same
   claim in `docs/guide/repository.md`'s gate table.
5. If the gate lands on the publish path, add it to `.workaholic/deployments/docs-site.md`'s
   Procedure so the record still matches the workflow.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A hand-edited generated page (one that `gen-docs` would rewrite) turns some CI or release job
  red; it can no longer travel to qfs.qmu.co.jp unnoticed.
- An unmodified tree is green — the check does not fire on a clean checkout.
- No document still claims that `gen-docs --check` runs in no CI job.

**Verification method** — the commands/tests/probes that prove them:

- On a scratch branch, append a line to `docs/drivers.md` and push: the chosen job fails, and
  its log names `gen-docs --check`.
- Revert and push again: the same job passes.
- `git grep -n "gen-docs --check"` reads consistently across `CLAUDE.md` and
  `docs/guide/repository.md`.

**Gate** — what must pass before approval:

- The whole existing CI matrix stays green on an unmodified tree.
- `release.yml`'s four-tarball path still publishes the Release.

## Considerations

- The three placements are not interchangeable and the choice is a real one — pick with the
  reasoning written down, not by convenience (`.github/workflows/ci.yml`,
  `.github/workflows/release.yml`).
- `gen-skills --check` and `check-migrations` sit in exactly the same position — documented as
  anti-drift, invoked by no CI job (`CLAUDE.md`). This ticket is scoped to `gen-docs` because
  that is the one the docs publish put at risk; whether the other two deserve the same
  treatment is a separate judgement and should not be folded in silently.
- `check-migrations` needs release tags to be meaningful and returns clean without them, so a
  blanket "run all three in CI" would add a check that quietly proves nothing on a shallow
  clone (`CLAUDE.md`).

## Final Report

Development completed as planned, but **step 1 falsified half the ticket's premise** and the
placement decision moved accordingly.

### What step 1 actually established

The ticket says "Nothing in CI runs `gen-docs --check`", citing `CLAUDE.md`. That sentence is
true about the *command* and false about the *property*. `packages/qfs/crates/qfs/src/docs.rs`
carries `docs::tests::committed_docs_match_generated_output`, a golden that calls the same
`check_docs()` the `--check` mode calls. It is an ordinary lib test, so it runs under
`cargo test --workspace`, so CI's `build-test` job already fails on drift.

Measured both halves on a branch checkout (2026-08-18), one line appended to `docs/drivers.md`:

| Probe | Clean tree | Hand-edited `docs/drivers.md` |
| --- | --- | --- |
| `cargo run -p xtask -- gen-docs --check` | `docs are in sync.`, exit 0 | `DRIFT — docs/drivers.md`, exit 1 |
| `cargo test -p qfs --lib docs::tests::committed_docs_match_generated_output` | pass | **FAILED**, `committed docs are stale: ["docs/drivers.md"]` |

`docs/guide/repository.md` had this right all along ("Enforced automatically by the docs-drift
golden test inside `qfs::docs`") while `CLAUDE.md` implied the opposite. The two documents
disagreed, and the ticket was written from the wrong one.

### The gap that is real, and where it is

`ci.yml` is `on: push: branches: ["**"]` plus `on: pull_request`. **A tag push matches neither** —
GitHub's `branches:` filter selects branch pushes only. So on a `v*` tag nothing from `ci.yml`
runs at all: not `build-test`, not the golden. `release.yml` is the entire tag pipeline, and it
had no drift check anywhere. That is the exact path the ticket cares about, because
`docs-deploy-production` publishes the tagged tree's `docs/` to qfs.qmu.co.jp alongside the
binary `install.sh` serves from the same tag.

The residual exposure is narrow — a tag is normally cut from a `main` commit that passed CI — but
it is not zero, and it is the only place where a drifted page can reach a public hostname.

### The placement decision, and why the other three lost

Chosen: **a new `docs-drift` job in `release.yml`, which `docs-deploy-production` `needs`.**

| Candidate | Rejected because |
| --- | --- |
| A new `gen-docs --check` job in `ci.yml` | The property is already gated there by the golden. This would add a second cold Rust build to every branch push to re-prove what `build-test` proves, and would leave the tag path — the one that publishes — still unguarded. |
| A step inside `docs-deploy-production` | Puts a Rust toolchain and an `xtask` build in front of a Node-only deploy job, on the critical path, and a failure reads as the deploy job breaking rather than as drift. |
| A gate on the `release` job | Refuses to publish a perfectly good binary release over a documentation defect. Too strong: the binary is not at fault. |

As its own job it runs in parallel with the four `build` matrix legs, so the tag pipeline's
critical path does not grow; a failure is a red check literally named `docs anti-drift (gen-docs
--check)`; and because only the docs publish `needs` it, the GitHub Release still goes out while
qfs.qmu.co.jp keeps serving the previously published version — which is exactly what
`docs-site.md`'s Failure section already promised for a red docs job.

### Changes

- `.github/workflows/release.yml` — new `docs-drift` job (checkout, `Swatinem/rust-cache` with its
  own `docs-drift` key, `rustup show`, `cargo run -p xtask -- gen-docs --check`);
  `docs-deploy-production` now `needs: [release, docs-drift]`. The job comment records the
  measurement, the three rejected placements, and what a red run looks like to whoever pushed
  the tag.
- `CLAUDE.md` — the misleading sentence is replaced by a per-property table saying which
  anti-drift property is defended on a branch/PR and which on a `v*` tag, and why the tag column
  exists at all. `gen-skills --check` and `check-migrations` are still undefended in both columns;
  that is stated rather than blurred.
- `docs/guide/repository.md` — the verification-surface section gains the tag-path paragraph, and
  the generator table's `gen-docs` row now names both enforcers instead of one.
- `.workaholic/deployments/docs-site.md` — the Production procedure gains `docs-drift` as step 2
  and the corrected `needs`, plus a "A red `docs-drift`" paragraph naming the recovery (regenerate
  on `main`, commit, re-tag — re-running the deploy job alone republishes the same drifted tree).

### Quality gate

| Criterion | Result |
| --- | --- |
| A hand-edited generated page turns some CI or release job red | **Pass**, on both paths: `build-test` red via the golden (branch/PR), `docs-drift` red via `gen-docs --check` exit 1 (tag). Both measured above. |
| An unmodified tree is green | **Pass** — `gen-docs --check` exit 0 and the golden passes on the reverted tree. |
| No document still claims `gen-docs --check` runs in no CI job | **Pass** — `git grep -n "gen-docs --check"` over `CLAUDE.md`, `docs/guide/repository.md`, `.github/` and `.workaholic/deployments/` reads consistently; the remaining hits are the developer-gate listing, the new job, and the two corrected passages. |
| The existing CI matrix stays green on an unmodified tree | **Pass** — `npm run docs:build` completes (`docs/guide/repository.md` is a built page); `cargo test --workspace` re-run on the branch; no Rust source was touched. |
| `release.yml`'s four-tarball path still publishes the Release | **Pass** — `build` and `release` are untouched, and `release` does not `need` the new job. Parsed the workflow with `yaml.safe_load`: jobs are `build`, `release`, `docs-drift`, `docs-deploy-production`, and only the last one's `needs` changed. |

### Discovered Insights

- **Insight**: `xtask gen-docs --check` and `qfs::docs::tests::committed_docs_match_generated_output`
  are the same check behind two front doors — both call `check_docs()` in
  `crates/qfs/src/docs.rs`. Only the front door differs, and only one of the two is reachable
  from CI on a branch.
  **Context**: "Is this guarded in CI?" cannot be answered by grepping the workflows for the
  command. Two of the three anti-drift generators really are unguarded; the third has a test
  standing in for it, and the repository's own two top-level documents disagreed about which was
  which for long enough to produce this ticket.

- **Insight**: `ci.yml`'s `release-artifacts` job is `if: startsWith(github.ref, 'refs/tags/')`
  inside a workflow whose only triggers are `push: branches: ["**"]` and `pull_request`. A tag
  push satisfies neither trigger, so that job cannot ever run.
  **Context**: Noticed while establishing that a tag runs nothing from `ci.yml`. It is dead
  configuration, superseded by `release.yml`'s `build` matrix, and it is not in this ticket's
  scope — minted as its own ticket rather than removed opportunistically.

- **Insight**: a job that gates a *publish* rather than a *build* belongs beside the publish and
  wired with `needs:`, not inside it. `docs-deploy-production` being skipped (not failed) is the
  desired outcome: the last good deploy keeps serving, and Cloudflare has no partial state to
  unwind.
  **Context**: `.workaholic/deployments/docs-site.md` already described this failure behaviour for
  the deploy job; the new gate had to be placed so that description stayed true instead of
  needing a rewrite.
