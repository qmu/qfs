---
created_at: 2026-08-17T14:24:43+00:00
status: done
author: noreply@anthropic.com
assignees: [a@qmu.jp]
depends_on: [20260817142443-a-release-publishes-the-docs-site-to-qfs-qmu-co-jp.md]
mission: the-documentation-site-publishes-itself-staging-on-merge-production-on-release
merge_policy:
verification_handoff:
---

# The docs deployment is recorded where the GitHub Release already is

## Overview

PROPOSED. Once the two triggers exist, the repository has a second deployment and says so
nowhere. `.workaholic/deployments/` holds exactly one record today — `github-release.md`,
"there is no separate server" — and `CLAUDE.md`'s Deploy section repeats that claim. Both
become false the moment the docs site publishes itself to two hostnames. This ticket writes
the deployment record for the docs site and corrects the two places that assert the old
shape, so the next person to ask "how does this deploy?" reads the answer instead of
reconstructing it from workflow YAML.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `.workaholic/deployments/github-release.md` — the existing record, and the shape to
  follow (frontmatter: title, environment, confirmation_method, url; then Procedure and
  Confirmation).
- `.workaholic/deployments/index.md` — the area index the new record joins.
- `CLAUDE.md` — its Deploy section states there is no separate deployment.
- `docs/guide/repository.md` — documents what CI proves; the publish step belongs there.

## Implementation Steps

1. Write `.workaholic/deployments/docs-site.md` covering both environments: trigger,
   procedure, the secrets by name, and how each is confirmed (the `curl` checks the two
   trigger tickets used).
2. Add it to `.workaholic/deployments/index.md`.
3. Correct `CLAUDE.md`'s Deploy section: the GitHub Release is still the binary
   deliverable, and the documentation site now deploys on merge and on release.
4. Note the two hostnames wherever the docs site is described for contributors
   (`docs/guide/repository.md`, and the docs README if it names only the local dev server).
5. State plainly which of the two records applies to which change, so a reader picking up a
   failed deploy knows which procedure they are in.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A deployment record for the docs site exists and names both environments, their triggers,
  their confirmation checks, and their secrets by name (never their values).
- No document still claims the repository has no separate deployment.
- The record's confirmation steps are the ones the workflows actually perform.

**Verification method** — the commands/tests/probes that prove them:

- `git grep -n "no separate server deployment"` returns only corrected prose.
- Read the new record against the merged workflow YAML: every step it names exists.
- The docs site production build stays green (`npm run docs:build`).

**Gate** — what must pass before approval:

- CI green, including the `docs-build` job (this ticket edits documentation pages, which
  that job compiles).

## Considerations

- This ticket is last on purpose: written before the workflows land, the record would
  describe an intention rather than a deployment, which is the failure mode a deployment
  record exists to prevent.

## Final Report

Development completed as planned. `.workaholic/deployments/docs-site.md` records both
environments — trigger, procedure, the two secrets by name, the by-hand recovery path, and a
per-environment confirmation check — and joins the area index. `CLAUDE.md`'s Deploy section now
carries both targets instead of asserting there is only one.

Two corrections went beyond the listed steps, both because the old prose became false rather
than merely incomplete:

- `.workaholic/deployments/github-release.md` opened with "there is no separate server". Left
  alone it would have contradicted the record filed next to it, so it now scopes itself to the
  binary and points at `docs-site.md` for the site. The claim it makes is narrowed, not
  reversed: no server is run for qfs, and that is still true.
- `docs/guide/repository.md`'s CI paragraph described `docs-build` as a compile check. That job
  now also publishes staging, so a contributor reading the old sentence would not know a merge
  deploys anything.

The ticket's fifth step — say plainly which record applies to which change — is answered in both
directions: `docs-site.md` has a *Which record applies to your change* paragraph, and
`github-release.md` says the tag it tells you to push also publishes the documentation.

The frontmatter carried `depends_on: a-release-publishes-the-docs-site-to-qfs-qmu-co-jp`, a
mission-ticket slug rather than a ticket filename, which `hooks/validate-ticket.sh` rejects. It
is corrected in place to the filename form. Its sibling
`20260817142443-a-merge-to-main-publishes-…` carried the same shape and is now archived with it
intact, as history.

### Discovered Insights

- **Insight**: `ship/scripts/read-deployments.sh` gives a record `has_confirmation: true` only
  when it has both a `confirmation_method` field *and* a non-empty `## Confirmation` body, and
  that flag is what `/ship` §1-4 halts on. A record with a beautifully written procedure and an
  empty Confirmation section is invisible to the gate that exists to catch exactly that.
  **Context**: `bash <src>/skills/ship/scripts/read-deployments.sh` is the cheap way to check a
  new record is actually seen — it reported `count 2, has_confirmation true` here.

- **Insight**: One record covers both hostnames because the frontmatter carries a single
  `environment`, so the file is filed as `production` and staging is documented in its body.
  **Context**: If staging ever needs to be a `/ship` target in its own right — something to
  release *to* rather than a byproduct of merging — it needs its own file, not a second
  frontmatter block. The current shape is right while staging is defined as "whatever `main`
  is" and nothing is promoted from it.

- **Insight**: `okf/scripts/refresh-index.sh` regenerates every area index from the documents
  present, so a new record is added to `deployments/index.md` by running it rather than by
  hand-editing between the `okf:generated` markers.
  **Context**: A hand-added line survives until the next refresh reorders or drops it; the
  script sorts by filename, which is why `docs-site.md` now precedes `github-release.md`.
