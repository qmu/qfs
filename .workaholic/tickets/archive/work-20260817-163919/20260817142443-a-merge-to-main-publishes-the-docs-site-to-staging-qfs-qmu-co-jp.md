---
created_at: 2026-08-17T14:24:43+00:00
status: done
author: noreply@anthropic.com
assignees: [a@qmu.jp]
depends_on: the-docs-site-has-a-worker-deploy-target-it-can-be-published-to
mission: the-documentation-site-publishes-itself-staging-on-merge-production-on-release
merge_policy:
verification_handoff: confirming the staging hostname serves the merged commit needs the live Cloudflare account and DNS
---

# A merge to main publishes the docs site to staging-qfs.qmu.co.jp

## Overview

PROPOSED. The first of the ask's two triggers: every merge to `main` publishes the built
documentation to staging-qfs.qmu.co.jp. `main` is this repository's continuously
auto-merged development branch, so staging tracks the tip of development — which is exactly
what makes it staging rather than a second production.

The build half already exists: CI's `docs-build` job runs `npm run docs:build` on every
push (added 2026-08-17, `84040bc`, because `vitepress dev` compiles pages on demand and a
broken page was invisible). This ticket adds the publish half on the `main` branch only,
reusing that build rather than adding a second one.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `.github/workflows/ci.yml` — the `docs-build` job this extends (or the new workflow that
  reuses its artifact); note `on: push: branches: ["**"]`, so the publish step must be
  guarded to `main` or it fires on every topic branch.
- `package.json` — the deploy command established by the target ticket.
- The worker config from the target ticket — the staging environment.

## Implementation Steps

1. Decide where the publish runs: a step in `docs-build` guarded by
   `if: github.ref == 'refs/heads/main'`, or a separate `docs-deploy-staging` workflow on
   `push: branches: [main]` that rebuilds. Prefer the guarded step — one build, one source
   of truth about what "the site" is.
2. Add the repository secrets the deploy needs (names fixed by the target ticket) and wire
   them into the job's `env`, with the least permissions the deploy accepts.
3. Publish to the **staging** environment only; make it impossible for this job to reach
   the production environment (explicit environment name, never a default).
4. Have the job log the deployed commit SHA, so the hostname's content is traceable to a
   merge without asking Cloudflare.
5. Confirm a merge to `main` results in staging-qfs.qmu.co.jp serving that commit.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A merge to `main` triggers exactly one docs publish, to staging.
- A push to any other branch, and any pull request, publishes nothing.
- The job fails loudly (red CI) when the deploy fails; it never passes silently.

**Verification method** — the commands/tests/probes that prove them:

- Inspect the workflow run for a merge commit on `main`: the publish step ran.
- Inspect a topic-branch run and a pull_request run: the publish step was skipped.
- `curl -sI https://staging-qfs.qmu.co.jp` returns 200 and the page carries the merged
  commit's content.

**Gate** — what must pass before approval:

- The whole existing CI matrix stays green and no job's duration regresses materially.
- The live-hostname check above is confirmed by the assignee (see `verification_handoff`).

## Open Decisions

- Whether staging-qfs.qmu.co.jp is publicly reachable or access-restricted — options:
  public (simplest; a search engine may index it and it will differ from the released
  docs) vs. restricted behind Cloudflare Access / a token (no leakage of unreleased
  documentation, one more credential to hold). The ask names the hostname and nothing about
  its audience, and neither answer is clearly right without knowing who is meant to read
  staging; the driving session resolves it explicitly and records the resolution.

## Considerations

- CI currently runs on `push: branches: ["**"]` *and* `pull_request`
  (`.github/workflows/ci.yml`), so an unguarded publish step would deploy from every branch
  — the guard is the whole safety of this ticket, not a detail.
- The `main` branch auto-merges propose/implement pull requests, so staging will move
  often; this is intended, and it is the reason production is tied to a tag instead.

## Final Report

Development completed as planned. The publish is a guarded pair of steps inside CI's existing
`docs-build` job — the ticket's preferred option — so one build produces both the compile
proof and the published bytes.

### Open Decision resolved: staging is public, and non-indexable

The ticket left open whether staging-qfs.qmu.co.jp is publicly reachable or access-restricted.
**Resolved: public, with crawling and indexing refused.**

The reasoning, recorded because the ticket required it rather than a silent pick: the two
options were weighed against what restriction would actually protect. This repository is
public, `main` is public, and the released documentation is public — so the content staging
serves is already readable by anyone who can read the repository. Access restriction would
therefore protect nothing that is not already open, while adding a second credential to hold
and a login between a contributor and the page they are checking. The one harm the ticket did
name is real, though: a search engine ranking unreleased documentation above the released
pages. That is an *indexing* problem, not an *access* problem, and it is answered directly —
`scripts/stamp-docs-deploy.sh staging` writes a `Disallow: /` `robots.txt` and an `_headers`
file setting `X-Robots-Tag: noindex, nofollow` on every path, both applied to the staging build
only. Production ships the tracked `docs/public/robots.txt` and stays indexable.

If the audience for staging later turns out to include people who must not read unreleased
documentation, this resolution is the thing to revisit — the environment is already declared
separately, so restricting it is a change to one hostname, not to the pipeline.

### Discovered Insights

- **Insight**: The guard is `github.event_name == 'push' && github.ref == 'refs/heads/main'`,
  not `github.ref` alone. For a `pull_request` event `github.ref` is the PR's merge ref
  (`refs/pull/<n>/merge`), so the ref check alone already skips PRs — but the event check is
  what keeps a fork whose head branch is literally named `main` from ever matching.
  **Context**: `.github/workflows/ci.yml` triggers on `push: branches: ["**"]` *and*
  `pull_request`, so every step added to a job in this workflow runs on far more events than a
  reader assumes. Anything with an outward effect needs both halves of the guard.

- **Insight**: The deployed tree carries `version.json` (commit, ref, environment, build time),
  written by the shared stamp script rather than by each workflow.
  **Context**: It makes "which commit is this hostname serving?" a `curl`, answerable by anyone
  without a Cloudflare login — which is what makes the confirmation check in the deployment
  record something a person can actually run. Both environments write it through the same
  script specifically so the staging and production stamps cannot drift apart.
