---
created_at: 2026-08-17T13:22:27+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [20260817132227-a-merge-to-main-deploys-the-docs-to-staging-qfs-qmu-co-jp.md]
mission: the-docs-site-publishes-itself-staging-on-merge-to-main-production-on-release
merge_policy:
verification_handoff: Confirming the production hostname serves the released docs needs the Cloudflare account and the qmu.co.jp DNS record, and a real release tag
---

# Cutting a release deploys the docs to qfs.qmu.co.jp

## Overview

The second half of the ask: the documentation that matches an installable binary is served at
`qfs.qmu.co.jp`. The ask phrases the trigger as "every merge that produces a release"; in this
repository a release is not produced by a merge but by pushing a `vX.Y.Z` tag, which fires
`.github/workflows/release.yml` (four native tarballs → a GitHub Release). So the production docs
deploy hangs off that same release cycle, which is the faithful reading of the ask against how
this project actually ships.

Landing after the staging ticket is deliberate: the same build-and-deploy chain has already been
proven end to end against a hostname whose breakage costs nothing.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` / `policies/ci-cd.md` — a check that is not automated is not a check
- `workaholic:operation` / `policies/security.md` — credentials live in secrets, never in the repo

## Key Files

- `.github/workflows/release.yml` — the tag-triggered pipeline; its `release` job is the natural
  place to hang the docs deploy, or to gate a separate workflow on.
- The docs-deploy workflow from the staging ticket — the production path should reuse it (a
  reusable workflow or a shared composite step), not be a second copy that drifts.
- `wrangler.toml` and `docs:deploy:production` from the first ticket.
- `CLAUDE.md` — the Deploy section's numbered release steps, which should now say that tagging
  also publishes the documentation.
- `install.sh` and `README.md` — wherever the docs URL is stated to users, it should be the
  production hostname once it is live.

## Implementation Steps

1. Choose and record the exact trigger: a job in `release.yml` (`push: tags: v*`), or a separate
   workflow on `release: types: [published]`. The first ties the docs to the tag, the second to
   the release actually being published; prefer running after the release job succeeds, so docs
   are not published for a release whose binaries failed to build.
2. Add the job: checkout the tag, `setup-node`, `npm ci`, `npm run docs:build`, deploy to the
   production environment. Reuse the staging ticket's workflow rather than duplicating it.
3. Confirm the job's permissions and secrets: it needs the Cloudflare token, and it must not
   widen `release.yml`'s existing `contents: write` scope for anything else.
4. Bind `qfs.qmu.co.jp` to the production worker and confirm TLS and DNS.
5. Make failure safe: a failed docs deploy must not fail or roll back the binary release, and must
   leave the previously deployed production site serving. Record which way that dependency runs.
6. Verify on the next real release tag, and record the release version the production site was
   first served from.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Pushing a `vX.Y.Z` tag publishes the documentation to `qfs.qmu.co.jp` with no manual step.
- The content served matches the tagged commit's `docs/`, not `main`'s.
- A failed docs deploy leaves the GitHub Release intact and the previously deployed site serving.
- The staging deploy is untouched by a release — the two environments never cross.

**Verification method** — the commands/tests/probes that prove them:

- The workflow run for a real release tag, URL and conclusion recorded.
- `curl -sS https://qfs.qmu.co.jp/` returns the tagged docs; a string present only in the tagged
  commit is found, and a string added to `main` after the tag is not.
- `curl -sS https://staging-qfs.qmu.co.jp/` still serves `main`'s content after the release run.
- `bash scripts/check-no-live-credentials.sh` exits 0.

**Gate** — what must pass before approval:

- A release tag produces a green docs-deploy run and the production hostname serves that release's
  documentation.
- `npm run docs:build` exits 0 locally.

## Considerations

- "Every merge that produces a release" is the ask's phrasing; this repository produces releases
  from tags, not merges. Step 1 records the reading actually implemented so the ask's author can
  disagree with it in review rather than discover it later.
- The docs describe the binary, so publishing them before the release's tarballs exist would point
  readers at install instructions for a release that has not appeared yet. Ordering the docs deploy
  after the release job avoids that window.
- If the docs ever need to be re-published without cutting a release (a typo fix on a live page),
  the production workflow should be `workflow_dispatch`-able against a tag — cheap to add here,
  awkward to retrofit.
