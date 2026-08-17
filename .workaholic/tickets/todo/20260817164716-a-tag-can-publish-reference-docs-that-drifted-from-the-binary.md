---
created_at: 2026-08-17T16:47:16+00:00
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
