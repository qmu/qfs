---
type: Mission
title: The current situation of qfs is documented as it actually stands
slug: the-current-situation-of-qfs-is-documented-as-it-actually-stands
status: active
merge_policy:
created_at: 2026-08-17T10:26:06+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
assignee:
predicted_hours:
actual_hours: 0.8
feedback: [20260817102418-add-a-full-documentation-of-the-current-situation-starting-with-a-mission-ticket.md]
tickets: []
stories: []
gate_type: documentation
gate_target: /guide/architecture
gate_assert: The docs site serves a current-architecture page that renders and reads correctly, naming the crates, the engine layering, the state stores and the faces the binary actually serves, with the date and source it was verified against.
claim: work-20260818-224038
---

# The current situation of qfs is documented as it actually stands

## Goal

A reader arriving today — human or agent — should learn what qfs **is now** from the
documentation rather than from the source. The repository documents its *intent* well
(`docs/blueprint.md`, per-section `implemented`/`blueprint`/`parked`) and its *usage* well
(guide, cookbook, the binary-generated `language`/`drivers`/`server` references). What is
missing is the current situation as a whole: nothing describes the built system's shape —
the 43 crates under `packages/qfs/crates/` and how they layer — and `packages/qfs-viewer/`
reaches the docs site only through two blueprint sections.

## Scope

Documentation of what exists, written against the source and the shipped binary and dated.
**Out of scope**: behaviour changes, rewriting the blueprint's intent sections, and new doc
tooling beyond what the survey ticket justifies.

## Experience

- A reader opens the docs site and finds the system as built — crates, layering, state
  stores, faces — on a page that names when and against what it was verified.
- A reader can tell of any page whether it is generated from the binary or hand-written,
  and what it does not cover.
- Nothing that states intent can be read as a statement of current fact.

## Acceptance

*Proposed sketch — approval replans this to drive-ready.*

- [x] **The documentation surface is mapped against what ships today**, page by page, with
      the gaps named rather than guessed. (#20260817102723-survey-the-documentation-surface-and-map-it-against-what-ships-today.md)
- [x] **The architecture as built is documented** from the source: the crate map, the
      engine's layering, the two state stores, and the faces the binary serves. (#20260817102723-document-the-architecture-as-built-the-crate-map-the-engine-layering-the-state-stores-and-the-faces.md)
- [x] **The repository as it stands is documented**: both packages including qfs-viewer, the
      gates, the anti-drift generators, and the release path. (#20260817102723-document-the-repository-as-it-stands-both-packages-the-gates-the-anti-drift-generators-and-the-release-path.md)

## Changelog

- 2026-08-17 — Proposed from issue #66 (feedback
  `20260817102418-add-a-full-documentation-of-the-current-situation-starting-with-a-mission-ticket.md`).
  Provisional: the ask asked for a strategy plus a first mission ticket; the direction
  decomposes, so it is proposed as a mission (the strategy artifact needs a target date the
  ask does not state).
- 2026-08-17 — ticket archived — 20260817102723-survey-the-documentation-surface-and-map-it-against-what-ships-today.md
- 2026-08-17 — ticket archived — 20260817102723-document-the-architecture-as-built-the-crate-map-the-engine-layering-the-state-stores-and-the-faces.md
- 2026-08-17 — ticket archived — 20260817102723-document-the-repository-as-it-stands-both-packages-the-gates-the-anti-drift-generators-and-the-release-path.md
- 2026-08-17 — All three tickets driven on work-20260817-104129: the documentation map, the architecture-as-built page and the repository page, each dated against 52b0410 / qfs 0.0.108 — work-20260817-104129.md
- 2026-08-17 — ticket archived — 20260817105331-the-cookbook-recipe-ratchet-two-places-name-does-not-exist.md
- 2026-08-17 — ticket archived — 20260817110309-the-docs-site-production-build-fails-on-blueprint-md.md
- 2026-08-17 — ticket archived — 20260817111530-the-qfs-viewer-gate-cannot-pass-where-bun-is-installed.md
- 2026-08-17 — run recorded (+0.6h) — run-20260817-124626
- 2026-08-17 — Three follow-ups closed: the recipe ratchet claim re-pointed at its real file, the docs site production build fixed and gated in CI, and the viewer gate's runtime coverage stated with a narrow dated bun exemption — work-20260817-124626.md
- 2026-08-17 — ticket archived — 20260817130410-seventy-seven-code-spans-across-the-docs-site-never-form.md
- 2026-08-17 — The unformed-code-span follow-up was withdrawn as a measurement error and the upstream plgg-md filing recorded blocked on qmu/plgg write access — work-20260817-132305.md
- 2026-08-18 — run recorded (+0.2h) — session_01FKn9idn6673AsxqXfeywjQ
- 2026-08-19 — ticket archived — 20260817131540-file-the-bun-plgg-md-parse-defect-upstream.md
- 2026-08-19 — concern deferred (stuck) — 20260819144514-ci-gained-two-third-party-setup.md
- 2026-08-19 — concern deferred (stuck) — 20260819144514-the-upstream-build-wart-is-real.md
