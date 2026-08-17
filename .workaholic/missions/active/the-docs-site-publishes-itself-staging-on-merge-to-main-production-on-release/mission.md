---
type: Mission
title: The docs site publishes itself: staging on merge to main, production on release
slug: the-docs-site-publishes-itself-staging-on-merge-to-main-production-on-release
status: active
merge_policy:
created_at: 2026-08-17T13:21:26+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
assignee:
predicted_hours:
actual_hours:
feedback: [20260817132004-auto-deploy-docs-to-staging-qfs-qmu-co-jp-on-merge-to-main-and-to-qfs-qmu-co-jp-on-release.md]
tickets: []
stories: []
gate_type:
gate_target:
gate_assert:
---

# The docs site publishes itself: staging on merge to main, production on release

## Goal

The documentation site (repo-root `docs/`, VitePress) is published nowhere. It is served only
by `docker compose up docs` on a developer's own machine, reachable from outside solely through
a personal cloudflared tunnel (`docs/.vitepress/config.mts` still whitelists `qfs-guide.qmu.dev`).
So the docs a reader is pointed at are whichever laptop happens to be running, and nothing
publishes what merged.

The direction asks for the two publications the project's own rhythm already implies: **every
merge to `main` deploys the docs to `staging-qfs.qmu.co.jp`** — the current, unreleased truth —
and **every release deploys the same docs to `qfs.qmu.co.jp`** — the truth that matches the
binary a user can install. The vehicle the ask names is a worker (Cloudflare), which is also
where this project's other hosting already points.

## Scope

In scope: a deployable docs artifact and its worker, the two triggers (push to `main`, and the
`v*` release cycle `release.yml` already runs), the two hostnames, and the credentials/secret
plumbing those need. Out of scope: the content of the docs themselves, the parked qfs-host
Workers entrypoint (a different artifact entirely), and any change to how releases are cut.

**Prerequisite, already met:** `npm run docs:build` failed on `docs/blueprint.md` when this
mission was proposed; the fix landed on `main` the same day (`84040bc`, ticket
`20260817110309-the-docs-site-production-build-fails-on-blueprint-md.md`, now archived), and
`ci.yml` gained a `docs-build` job that walks the production build on every push. So a green
build exists to deploy, and this mission builds on that gate rather than duplicating it.

## Experience

A contributor merges a pull request to `main`; a few minutes later `staging-qfs.qmu.co.jp`
serves the merged documentation, with no one having run a command. The developer cuts the next
release the usual way (`git tag -a vX.Y.Z && git push origin vX.Y.Z`); when `release.yml`
publishes the GitHub Release, `qfs.qmu.co.jp` serves that same documentation. A failed docs
build fails the run loudly and leaves the previously deployed site untouched, so neither
hostname can go blank or serve a half-built site.

## Acceptance

- [ ] A merge to `main` publishes the built docs to `staging-qfs.qmu.co.jp` with no manual step,
      and the deployed site's content matches the merge commit. (#20260817132227-a-merge-to-main-deploys-the-docs-to-staging-qfs-qmu-co-jp.md)
- [ ] The `v*` release cycle publishes the same built docs to `qfs.qmu.co.jp`, and a release that
      does not run leaves the production site as it was. (#20260817132227-cutting-a-release-deploys-the-docs-to-qfs-qmu-co-jp.md)
- [ ] The docs build is gated before either deploy: a broken `npm run docs:build` fails the
      workflow instead of publishing, and no credential is committed to the repository. (#20260817132227-the-docs-build-produces-a-deployable-artifact-and-a-worker-that-serves-it.md)

## Changelog

- 2026-08-17 — Proposed from feedback `20260817132004-auto-deploy-docs-to-staging-qfs-qmu-co-jp-on-merge-to-main-and-to-qfs-qmu-co-jp-on-release.md` (issue #69).
