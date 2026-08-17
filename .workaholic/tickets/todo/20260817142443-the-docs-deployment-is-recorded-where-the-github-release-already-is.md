---
created_at: 2026-08-17T14:24:43+00:00
author: noreply@anthropic.com
assignees: [a@qmu.jp]
depends_on: a-release-publishes-the-docs-site-to-qfs-qmu-co-jp
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
