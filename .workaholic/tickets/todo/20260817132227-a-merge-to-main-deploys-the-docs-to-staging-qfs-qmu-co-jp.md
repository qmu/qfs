---
created_at: 2026-08-17T13:22:27+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [20260817132227-the-docs-build-produces-a-deployable-artifact-and-a-worker-that-serves-it.md]
mission: the-docs-site-publishes-itself-staging-on-merge-to-main-production-on-release
merge_policy:
verification_handoff: Confirming the staging hostname serves the merged docs needs the Cloudflare account and the qmu.co.jp DNS record, neither available to an unattended run
---

# A merge to main deploys the docs to staging-qfs.qmu.co.jp

## Overview

The first half of the ask: every merge to `main` publishes the documentation to
`staging-qfs.qmu.co.jp`, with nobody running a command. `.github/workflows/ci.yml` already runs
on every push and pull request but has no docs job at all; `release.yml` fires only on a `v*` tag.
So the trigger this needs does not exist yet, and neither does the credential plumbing.

This ticket adds only the staging trigger and its secrets, on top of the worker and scripts the
previous ticket lands. Staging is deliberately first: it is the environment whose breakage costs
nothing, and it proves the whole chain — build, authenticate, deploy, serve — before the same
chain is pointed at the public hostname.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` / `policies/ci-cd.md` — a check that is not automated is not a check
- `workaholic:operation` / `policies/security.md` — credentials live in secrets, never in the repo

## Key Files

- `.github/workflows/` — a new docs-deploy workflow (or a job added to `ci.yml`; step 1 decides),
  triggered by `push` on `main` only.
- `wrangler.toml` and the `docs:deploy:staging` script from the previous ticket — this workflow
  should call them, not re-implement the deploy.
- `.github/workflows/ci.yml` — the reference for this repository's runner conventions
  (`actions/checkout@v4`, per-job cache keys, `defaults.run.working-directory`); the docs job runs
  at the repository root, not under `packages/qfs`.
- `CLAUDE.md` — the Deploy section, which currently states the deliverable is the GitHub Release
  only and must now also describe the docs deployment.

## Implementation Steps

1. Decide and record where the job belongs: a separate `docs-deploy.yml` triggered on
   `push: branches: [main]`, or a `main`-guarded job inside `ci.yml`. Prefer the separate file if
   the deploy needs permissions or secrets the rest of CI should not carry.
2. Write the workflow: checkout, `setup-node` (pin the same major the docs toolchain expects),
   `npm ci` at the repository root, `npm run docs:build`, then the staging deploy. The build step
   is separate from the deploy step so a build failure is legible as a build failure.
3. Wire the credential: `CLOUDFLARE_API_TOKEN` (and account id, if the worker declaration needs
   one) as repository secrets, referenced by name and never echoed. Scope the token to the
   minimum the deploy needs.
4. Bind the custom domain `staging-qfs.qmu.co.jp` to the staging worker, and confirm the DNS
   record resolves and serves over TLS.
5. Guard against a partial publish: the deploy step runs only when the build succeeded, and a
   failed run must leave whatever was previously deployed still serving.
6. Verify end to end by merging one real, trivial docs change and watching it appear.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A merge to `main` triggers the workflow with no manual step, and it exits green.
- `https://staging-qfs.qmu.co.jp/` serves the documentation built from that merge commit — a
  string added by the test change is present in the served HTML.
- A run whose `npm run docs:build` fails does not deploy, and the previously deployed site is
  unchanged.
- No credential appears in the workflow file, the logs, or the repository.

**Verification method** — the commands/tests/probes that prove them:

- The workflow run's URL and conclusion for the merge commit, recorded.
- `curl -sS https://staging-qfs.qmu.co.jp/ | grep <the test string>` after the run completes.
- A scratch branch with a deliberately broken docs page, merged to a test target or run through
  `workflow_dispatch`, showing the deploy step skipped and the site still up.
- `bash scripts/check-no-live-credentials.sh` exits 0.

**Gate** — what must pass before approval:

- The workflow is green on `main` and the staging hostname serves the merged content.
- `npm run docs:build` exits 0 locally.

## Considerations

- Every merge to `main` deploys, and `main` is this project's continuously auto-merged development
  branch — so staging will redeploy often. Concurrency matters: use a `concurrency` group so two
  merges landing close together cannot publish out of order.
- The docs build does not need the Rust toolchain, so this job must not pull the Cargo cache; a
  docs deploy that waits on a Rust build would be slow for no reason.
- The generated reference pages (`docs/{language,drivers,server}.md`) are committed, so the docs
  build needs no `cargo run -p xtask -- gen-docs` step. If that ever changes, this workflow becomes
  a Rust job too — worth a comment in the file so the next reader knows why it is absent.
