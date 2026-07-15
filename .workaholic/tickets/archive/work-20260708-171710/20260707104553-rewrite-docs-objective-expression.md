---
created_at: 2026-07-07T10:45:53+09:00
author: a@qmu.jp
type: housekeeping
layer: [UX, Domain]
effort:
commit_hash: d0ca721
category: Changed
depends_on:
mission:
---

# Rewrite qfs documentation expression objectively

## Overview

The qfs documentation now reflects the current design more closely, but many headings and paragraphs
still use conversational, promotional, or evaluative expressions. This ticket plans and executes a
full expression pass over the documentation so titles identify the subject being documented and body
text states observable behavior, constraints, and verification facts.

The rewrite should cover hand-written docs, generated docs, and the source templates that produce
generated docs. It should not only patch individual phrases; it should make the documentation follow
a repeatable title and wording rule that can be checked in review.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — keep hand-written docs,
  generated docs, source templates, and skill artifacts in their existing ownership boundaries.
- `workaholic:implementation` / `policies/coding-standards.md` — documentation must describe the
  typed/domain behavior as implemented, without relying on vague wording to cover missing precision.
- `workaholic:implementation` / `policies/objective-documentation.md` — titles and prose must use
  factual, verifiable language and avoid marketing-inflected or aspirational claims.
- `workaholic:design` — documentation is part of the operator and agent interface; headings should
  make current capabilities, constraints, and failure modes discoverable without interpretive effort.

## Quality Gate

Acceptance criteria:

- Every `docs/**/*.md` heading names a documented subject, command, resource, state, or operation.
  Headings do not use slogans, rhetorical questions, emojis, reaction-oriented phrases, or status
  decoration such as "just shipped" or "see it work first".
- Evaluative words such as "simple", "powerful", "safe", "honest", "real", "easy",
  "intuitive", "magic", and "seamless" are removed unless the same sentence states the concrete
  invariant, check, or command output that makes the claim verifiable.
- Generated documentation is corrected at its source template or generator input. Hand edits are not
  made to generated files when a generator owns the text.
- The table of contents and page titles use a consistent taxonomy by doc type: concept pages name
  concepts, reference pages name commands/resources, cookbook pages name tasks, and operational pages
  name runtime state or procedure.
- The documentation still covers the current qfs design after the expression rewrite: defined paths,
  accounts, OAuth app labels, `/sys`, DDL/config event history, dump/restore, server bindings, and
  preview/commit gates remain documented.

Verification method:

- Run a heading scan over `docs/**/*.md` and confirm no non-objective title patterns remain outside
  explicitly justified allowlist entries.
- Run a wording scan over `docs/**/*.md` for the evaluative terms listed above and record each
  remaining occurrence with its objective justification or rewrite it.
- `cargo run -p xtask -- gen-docs --check`
- `cargo run -p xtask -- gen-skills --check`
- `cargo test -p qfs docs::tests`
- `npm run docs:build`

Gate:

- All commands above pass, and manual review confirms headings and body text can be judged from
  observable behavior rather than tone or intended reader reaction.

## Key Files

- `docs/` — primary documentation tree that needs the full expression audit.
- `docs/index.md`, `docs/README.md`, `docs/blueprint.md`, `docs/roadmap.md` — pages with the highest
  concentration of title and tone drift.
- `docs/cookbook/*.md` — task pages that repeatedly use conversational headings and safety claims.
- `docs/guide/*.md` — operator and concept pages where headings should become stable subject names.
- `docs/language.md`, `docs/server.md`, `docs/drivers.md`, `docs/query-cookbook.md` — generated or
  generator-backed docs; source text must be updated through the owning generator path.
- `crates/qfs/src/docs.rs`, `crates/qfs/src/skill_docs.rs`, `crates/qfs/src/server.rs`, `crates/parser`
  — likely source locations for generated prose and checked docs.
- `xtask` — documentation and skill generation checks.

## Related History

- `.workaholic/tickets/archive/work-20260707-025845/20260707034922-reorganize-qfs-docs-design-snapshot.md`
  reorganized the docs around the current design. This ticket follows that work by correcting the
  expression layer.
- `.workaholic/tickets/archive/work-20260629-110121/20260629110939-rewrite-docs-to-shipped-reality-erase-roadmap.md`
  and related docs honesty tickets removed stale capability claims. This ticket extends the same
  discipline to headings, titles, and subjective language.

## Implementation Steps

1. Inventory all documentation pages and classify each file as hand-written, generated, or generated
   from checked source data.
2. Define a local heading taxonomy before editing: concept, reference, cookbook task, operational
   procedure, security model, and roadmap/backlog. Use the taxonomy to decide replacement headings.
3. Rewrite headings first so each page outline is objective without reading body text.
4. Rewrite body prose to replace subjective claims with observable behavior, commands, outputs,
   invariants, or explicit limitations.
5. Update generator-owned source text for generated docs and regenerate outputs instead of hand
   editing generated files.
6. Add or document a repeatable expression audit command that scans headings and subjective wording.
   If a full lint tool is too large for this ticket, record the exact `rg` command and allowlist in
   the docs maintenance notes.
7. Run the documentation generation, skill generation, docs tests, and docs build checks in the
   Quality Gate.

## Considerations

- The qmu.co.jp objective documentation policy should be strengthened in parallel. It currently
  rejects evaluative adjectives in body text, but it does not explicitly govern titles, heading
  taxonomies, emoji, rhetorical questions, status labels, or generated documentation sources.
- Do not erase necessary risk language. Terms such as "safe" or "irreversible" can remain when tied
  to concrete gates like preview, commit, `--commit-irreversible`, or a documented failure mode.
- Avoid introducing a second documentation style guide inside qfs. Any durable rule should eventually
  live in the qmu policy and be referenced here through the Workaholic policy mirror.
- Keep examples runnable and current while changing tone. This ticket is not only a copy edit if
  source examples or generated prose need updates to keep checks green.
