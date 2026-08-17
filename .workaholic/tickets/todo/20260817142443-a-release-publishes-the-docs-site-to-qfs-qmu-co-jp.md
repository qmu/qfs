---
created_at: 2026-08-17T14:24:43+00:00
author: noreply@anthropic.com
assignees: [a@qmu.jp]
depends_on: the-docs-site-has-a-worker-deploy-target-it-can-be-published-to
mission: the-documentation-site-publishes-itself-staging-on-merge-production-on-release
merge_policy:
verification_handoff: confirming the production hostname serves the released docs needs the live Cloudflare account and a real tag push
---

# A release publishes the docs site to qfs.qmu.co.jp

## Overview

PROPOSED. The ask's second trigger: "every merge that produces a release (the binary
release cycle)" publishes the documentation to qfs.qmu.co.jp. In this repository a release
is not a merge — it is the `v*` tag pushed after the merge, which is what
`.github/workflows/release.yml` fires on and what `.workaholic/deployments/github-release.md`
documents as the deploy procedure. So the trigger implemented here is the release workflow,
which is the same event the ask means by "produces a release": a merge that is never tagged
produces no release and must publish nothing to production.

Tying production to the tag also gives the two hostnames their meaning: qfs.qmu.co.jp
carries the documentation of the version `install.sh` will actually install, while staging
carries the tip of `main`.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `.github/workflows/release.yml` — `on: push: tags: ["v*"]`; the docs publish joins it as
  its own job.
- `package.json` — `docs:build` and the production deploy command.
- The worker config from the target ticket — the production environment.
- `.workaholic/deployments/github-release.md` — the release procedure this extends; the
  recording ticket updates it.

## Implementation Steps

1. Add a `docs-deploy-production` job to `release.yml`, on the same `v*` tag trigger,
   independent of the per-target build matrix (documentation does not need the binaries).
2. Build the site in that job (`npm install && npm run docs:build`) from the tagged commit,
   so the published documentation is the tag's, never `main`'s at deploy time.
3. Publish to the **production** environment explicitly, with its own secrets.
4. Decide and implement the ordering relative to the GitHub Release: publish docs only
   after the release job succeeds, so a failed binary release does not advertise a version
   nobody can install (`needs:` the release job).
5. Confirm a real `v0.0.x` tag results in qfs.qmu.co.jp serving that tag's documentation.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A pushed `v*` tag publishes the tagged commit's documentation to qfs.qmu.co.jp.
- A merge to `main` with no tag publishes nothing to production.
- A failed release build does not publish documentation.

**Verification method** — the commands/tests/probes that prove them:

- Inspect the workflow run for the next `v0.0.x` tag: the docs job ran after the release
  job and reported the deployed commit.
- `curl -sI https://qfs.qmu.co.jp` returns 200, and the served docs match the tag (e.g. the
  version the guide quotes).
- Inspect a `main` merge run: no production publish appears.

**Gate** — what must pass before approval:

- `release.yml`'s existing four-tarball path is unchanged and still publishes the Release.
- The live-hostname check above is confirmed by the assignee (see `verification_handoff`).

## Considerations

- The generated reference pages (`docs/{language,drivers,server}.md`) are rendered from the
  binary by `cargo run -p xtask -- gen-docs`, and no CI job runs `gen-docs --check`
  (`CLAUDE.md`) — so a tag can ship documentation that drifted from the binary it
  documents. Publishing production docs from the tag makes that drift publicly visible;
  whether to add the check to this job is worth raising in the driving session, but it is
  a separate ticket's decision, not this one's.
- `release.yml` uses only the auto-provided `GITHUB_TOKEN` today; this job is the first
  secret consumer in that workflow, so secret scope should be set on the job, not the
  workflow.
