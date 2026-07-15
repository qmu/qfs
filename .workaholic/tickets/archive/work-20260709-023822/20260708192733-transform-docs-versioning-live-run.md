---
created_at: 2026-07-08T19:27:33+09:00
author: a@qmu.jp
type: enhancement
layer: [Config, Domain]
effort:
commit_hash: 69b4db5
category: Added
depends_on: [20260708192732-transform-execution-routing.md]
mission:
---

# Finish transform: docs, skills, version bumps, Decision-K sweep, live run

## Findings / Outcome (2026-07-09 night drive)

**Hermetic half: DONE.**
- **TRANSFORM grammar** added to the EBNF source (`crates/lang/src/reference.rs`): a
  `transform_stage` in `query_stage` (`"transform" , name`, flagged CONTEXTUAL — the closed core
  stays 39 keywords) plus a `transform_def` under `ddl` (`create transform … input(…) output(…)
  provider … model … [effort …] [secret …]`). Regenerated `docs/language.md` via `gen-docs`; the
  `language_doc_carries_frozen_keywords_and_grammar` + `grammar_uses_only_frozen_vocabulary` tests
  stay green (the drift test only requires frozen keywords to APPEAR — it does not forbid the
  contextual `"transform"` terminal).
- **Cookbook recipe** added to `docs/cookbook/automation.md` ("Transform — call a model over
  rows"): declare → use in a pipeline → retire, all parse-checked by `cookbook_skills.rs`
  (138 recipes green). Commit is expressed as the `--commit --commit-irreversible` CLI flag (the
  cookbook convention), not an inline keyword. Regenerated skills via `gen-skills`.
- **Decision-K sweep**: all ten doc-comment sites + the two `Cargo.toml` descriptions re-pointed
  from "qfs NEVER hosts or calls an LLM" (decision K) to blueprint §15 / decision W — the `/claude`
  driver remains a model-free façade, but the blanket global claim is retired (qfs DOES call a
  model via `|> transform`, behind the injected provider in `qfs-driver-transform` + the binary,
  never the pure engine). `grep -n "NEVER hosts or calls"` over `packages/` is clean; the only
  remaining "decision K" mention is the intentional supersession note in `driver-claude/src/lib.rs`.
- **Version bumps**: qfs patch `0.0.37 → 0.0.38`; the four plugin `version` fields (minor, new
  taught surface) `0.6.2 → 0.7.0` (`plugins/qfs/.claude-plugin/plugin.json`,
  `plugins/qfs/.codex-plugin/plugin.json`, both fields in `.claude-plugin/marketplace.json`).
- All four ratchets green (gen-docs/gen-skills/check-migrations + cookbook parse-check); clippy
  clean; fmt clean.

**Live-provider run (step 6): DEFERRED — owner-gated, not run.**
This is the single non-hermetic check. It requires (a) explicit owner go-ahead in-session and (b)
authenticating a REAL model provider and spending real tokens. This dev host has LIVE cloud
accounts connected (`.claude` memory: qfs-env-has-live-cloud-accounts), so an autonomous night run
must NOT perform it. The binary today wires the fail-closed `UnconfiguredProvider` (a transform
COMMIT refuses with an actionable "no model provider configured" error), so no live provider is
even bound yet — binding one is part of this owner-approved step. **The feature is NOT fully
approvable until the owner runs one PREVIEW (record the cost estimate) + one COMMIT with the
irreversible ack (record the observed output) against an authenticated provider and pastes the
evidence here.** Everything else ships; this is the one morning-review action.

## Overview

Last of four dependency-ordered transform tickets (supersedes the deleted mega-ticket
`20260708002200`). With the definition (T1), plan spine (T2), and execution/routing (T3) landed,
this ticket makes the taught surface true and complete: the generated language reference gains the
TRANSFORM rule, a cookbook recipe teaches it (parse-checked), skills regenerate, the four plugin
version fields take the **minor** bump (taught-surface change) plus the qfs patch bump for the
shipping PR, the ten Decision-K citation sites are re-pointed to blueprint §15, and the **one
recorded live-provider run** — the single manual, out-of-band check of the whole feature — is
executed and its evidence recorded here.

**Discovery state (HEAD 24c2269):** `docs/language.md` does **not** render a TRANSFORM rule (only
two incidental "transform" words at `:6`, `:96`); the Decision-K sweep list is confirmed at exactly
**ten** doc-comment sites (below) plus two `Cargo.toml` description mentions worth reviewing.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — generated docs change at their
  Rust source (`docs.rs` / `grammar_ebnf`), never by hand-editing output; cookbook is the skill
  source.
- `workaholic:implementation` / `policies/coding-standards.md` — doc-comment sweep keeps citations
  accurate to the governing decision (§15), not a retired one (Decision K).
- `workaholic:implementation` / `policies/objective-documentation.md` — docs state implemented,
  verifiable behavior only; the cookbook recipe must parse and run against the shipped binary
  (the verified-true ratchet).
- `workaholic:operation` / `policies/ci-cd.md` — the four anti-drift ratchets (gen-docs,
  gen-skills, check-migrations, cookbook parse-check) all stay green; version bumps ride the same
  PR as the surface change (CLAUDE.md rule).

## Key Files

Verified anchors at HEAD `24c2269` (2026-07-08):

- `packages/qfs/crates/qfs/src/docs.rs:80` — `render_language` embeds `grammar_ebnf()`; the
  TRANSFORM EBNF rule is added at the `grammar_ebnf` source (in `crates/lang`/parser side), then
  `cargo run -p xtask -- gen-docs` regenerates `docs/language.md` (never hand-edit it). The
  `language_doc_carries_frozen_keywords_and_grammar` test (docs.rs:432) must stay green.
- `docs/cookbook/` + `packages/qfs/crates/test/tests/cookbook_skills.rs` — one recipe using
  `transform` (likely in `automation.md` or a fitting article), parse-checked by the ratchet;
  `cargo run -p xtask -- gen-skills` regenerates the SKILL.md files.
- **The ten Decision-K citation sites** (re-point "qfs NEVER hosts or calls an LLM" / Decision K →
  blueprint §15, Decision W):
  1. `packages/qfs/crates/driver-claude/src/lib.rs:11` (also `:104,:178,:229`)
  2. `packages/qfs/crates/driver-claude/src/schema.rs:10` (also `:36`)
  3. `packages/qfs/crates/driver-claude/src/applier.rs:11`
  4. `packages/qfs/crates/driver-claude/src/backend.rs:14` (also `:21`)
  5. `packages/qfs/crates/qfs/src/claude.rs:13` (also `:52,:238`)
  6. `packages/qfs/crates/qfs/src/catalog.rs:87`
  7. `packages/qfs/crates/qfs/src/describe.rs:275`
  8. `packages/qfs/crates/qfs/src/commit.rs:307`
  9. `packages/qfs/crates/mcp/src/lib.rs:16`
  10. `packages/qfs/crates/cmd/tests/dep_direction.rs:322`
  Plus review (not doc-comments): `packages/qfs/crates/qfs/Cargo.toml:172`,
  `packages/qfs/crates/driver-claude/Cargo.toml:9`.
- Version fields: `plugins/qfs/.claude-plugin/plugin.json`, `plugins/qfs/.codex-plugin/plugin.json`,
  both `version` fields in `.claude-plugin/marketplace.json` (**minor** bump — taught-surface
  change), and the qfs patch in `packages/qfs/crates/qfs/Cargo.toml` (per-shipped-PR rule).

## Related History

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) — the Decision-K reversal this sweep records in the code.
- `docs/blueprint.md:563` (§15, Decision W) — the citation target.
- CLAUDE.md — plugin re-versioning rule (four fields, minor for a taught-surface break) and the
  per-shipped-PR patch bump.

## Implementation Steps

1. Add the TRANSFORM rule to the EBNF source and regenerate `docs/language.md` via `gen-docs`;
   keep the frozen-keyword/grammar doc test green.
2. Write one cookbook recipe exercising `transform` end to end (declare → describe → preview →
   commit with the ack); confirm `cookbook_skills.rs` parse-checks it; regenerate skills via
   `gen-skills`.
3. Sweep the ten Decision-K citation sites to cite blueprint §15 (Decision W); review the two
   `Cargo.toml` descriptions for the same stale claim.
4. Bump the four plugin version fields (**minor**) and the qfs patch version in the shipping PR.
5. Run all four ratchets (`gen-docs --check`, `gen-skills --check`, `check-migrations`, cookbook
   parse-check) plus the workspace gate.
6. **Live run (with explicit owner go-ahead, out of band):** authenticate a real provider, run one
   `/source |> transform <def> |> …` PREVIEW (record the cost estimate) then COMMIT with the
   irreversible ack (record the observed output); paste both as evidence into this ticket's
   Findings section. This is the single non-hermetic check and never enters CI.

## Quality Gate

Distributed from the parent mega-ticket's gate (owner-approved 2026-07-08). This ticket carries the
**live-provider** half of the parent's division of assurance.

**Acceptance criteria:**

- `docs/language.md` (regenerated, never hand-edited) carries the TRANSFORM grammar rule;
  `cargo test -p qfs docs::tests` green.
- One cookbook `transform` recipe exists and is parse-checked green by
  `crates/test/tests/cookbook_skills.rs`; skills regenerated and in sync.
- All ten Decision-K doc-comment sites cite blueprint §15; no remaining "NEVER hosts or calls an
  LLM" claim contradicting the shipped behavior (grep-clean).
- Four plugin `version` fields minor-bumped together; qfs `Cargo.toml` patch-bumped in the same PR.
- **Live evidence recorded on this ticket:** the PREVIEW cost estimate and the committed run's
  observed output against a real authenticated provider, exactly once.

**Verification method:**

- `cargo run -p xtask -- gen-docs --check` / `gen-skills --check` / `check-migrations`;
  `cargo test -p qfs-test --test cookbook_skills`; `cargo test -p qfs docs::tests`;
  `clippy --workspace --all-targets -D warnings`; `fmt --all --check`;
  `rg -n "NEVER hosts or calls" packages/` returns only §15-citing text.
- Manual: the recorded live run (step 6), performed only after explicit owner approval in-session.

**Gate:** every ratchet green + the live evidence pasted into this ticket. Without the live run the
feature is not approvable (the parent gate's explicit condition).

## Considerations

- Depends on `20260708192732` (and transitively T1/T2): docs may only describe implemented
  behavior (`objective-documentation`); writing the recipe before execution lands would recreate
  the docs-ahead-of-code failure the roadmap cycle fixed.
- The live run spends real tokens and touches a live provider account — this dev host has live
  cloud credentials (`.claude` memory: qfs-env-has-live-cloud-accounts); ask the owner before the
  run, keep it to one PREVIEW + one COMMIT, and never paste secrets into the evidence.
- The plugin bump is **minor** here (new taught surface), unlike the patch bumps of routine PRs —
  all four fields move together (CLAUDE.md).
- If the split tickets ship as multiple PRs, each shipping PR still takes its own qfs patch bump;
  the minor plugin bump belongs to the PR that regenerates the skills (this one).
