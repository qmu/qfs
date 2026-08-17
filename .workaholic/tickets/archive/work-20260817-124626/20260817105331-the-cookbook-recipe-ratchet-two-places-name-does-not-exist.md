---
created_at: 2026-08-17T10:53:31+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff:
---

# The cookbook recipe ratchet two places name does not exist

## Overview

Found while surveying the documentation surface (ticket
`20260817102723-survey-the-documentation-surface-and-map-it-against-what-ships-today.md`).

`CLAUDE.md` states that every `qfs` recipe in a `docs/cookbook/*.md` article goes through a
verified-true ratchet at `crates/qfs/tests/cookbook_skills.rs`, holding it to two checks — it
parses on the shipped grammar, and the columns it names exist on the node it addresses — and
concludes "a skill can never teach an agent a statement the binary rejects, nor a column the
driver does not carry". `packages/qfs/crates/cmd/tests/faq_cli_surface.rs` refers to the same
file, placing it in `crates/test/`.

**No such file exists anywhere in the tree at `52b0410`.** `packages/qfs/crates/qfs/tests/` does
not exist; `crates/test/tests/` holds `dev_only_dep_graph.rs`, `harness_demo.rs`,
`planner_e2e_consumer.rs`, `roadmap_cookbook.rs` and `wasm_gating.rs`. Nothing else covers the
gap: `xtask::gen_skills` does no parsing at all (it renders and diffs text), and
`crates/skill/tests/golden_corpus.rs` proves the examples embedded in
`crates/skill/assets/SKILL.md`, not the cookbook articles. `roadmap_cookbook.rs` parse-checks
`docs/query-cookbook.md`, which is a different file with a different contract (target grammar,
tagged recipes).

So the 14 generated Agent Skills currently ship with no parse or column check over their
recipes, while two places in the repository say they are ratcheted. Step 1 is to establish which
of the two is true — the ratchet was never written, or it existed and was lost — before deciding
between restoring it and correcting the claim.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:implementation` / `policies/test.md` — what a test must actually prove
- `workaholic:implementation` / `policies/objective-documentation.md` — a claim in a document
  must be verifiable

## Key Files

- `CLAUDE.md` — the claim, in the "Build & test" section.
- `packages/qfs/crates/cmd/tests/faq_cli_surface.rs` — the second reference, and the model to
  follow: an integration test that walks a real in-binary surface and fails on drift.
- `packages/qfs/crates/test/tests/roadmap_cookbook.rs` — the existing cookbook-parsing ratchet
  for `docs/query-cookbook.md`; its extractor and ratchet-floor pattern are directly reusable.
- `packages/qfs/xtask/src/gen_skills.rs` — the generator whose output the claim is about; it
  reads the same articles and already locates every ```` ```qfs ```` block's source.
- `packages/qfs/crates/qfs/src/describe.rs` — the cred-free describe registry a column check
  would resolve names through.

## Implementation Steps

1. Establish whether the file ever existed: `git log --all --diff-filter=D --name-only` over
   `**/cookbook_skills.rs`, and search the archive for the ticket that introduced the claim.
   Record what is found — a lost file is restored, a claim that was never true is corrected.
2. If it existed, restore it at the path `CLAUDE.md` names and re-point `faq_cli_surface.rs`'s
   comment at the real location.
3. If it never existed, write it: extract every ```` ```qfs ```` recipe from each
   `docs/cookbook/*.md` article, assert it parses with `qfs_parser::parse_statement`, and
   resolve each column it names against the cred-free describe registry for the node the
   statement addresses — the two checks `CLAUDE.md` already promises.
4. Either way, put the check somewhere `cargo test --workspace` runs it, so CI defends it (see
   Considerations: no CI step invokes `xtask`).
5. Correct `CLAUDE.md` so its description matches whatever now exists, including where the test
   lives and what it does not cover.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A test exists at the path `CLAUDE.md` names, and it fails when a cookbook article is edited to
  contain a `qfs` statement the shipped grammar rejects.
- It also fails when a recipe names a column the addressed node's describe report does not
  carry.
- `CLAUDE.md`'s description of the ratchet resolves to a real file, as does the comment in
  `faq_cli_surface.rs`.
- Any recipe the new check cannot cover is named in the test's own doc comment rather than
  passing silently.

**Verification method** — the commands/tests/probes that prove them:

- `cd packages/qfs && cargo test --workspace` is green, and the new test is in its output.
- Two negative probes: temporarily append a deliberately invalid statement to one article and
  confirm the test fails; append a statement naming a non-existent column and confirm it fails.
  Revert both.
- `rg -n 'cookbook_skills' .` — every hit resolves to an existing path.

**Gate** — what must pass before approval:

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check` and
  `gen-skills --check` all exit 0.

## Considerations

- The claim is load-bearing for agents, not just for readers: the generated skills are installed
  into Claude Code caches, so an unchecked recipe teaches a statement the binary rejects with no
  feedback path back to the article (`plugins/qfs/skills/*/SKILL.md`).
- `.github/workflows/ci.yml` never invokes `xtask`, so a check placed only behind
  `gen-skills --check` would not be enforced anywhere; the `docs_drift_golden` unit test in
  `packages/qfs/crates/qfs/src/docs.rs` is the pattern that does get enforced.
- The column half of the check is the harder half and may need the fixture catalog/repo the
  golden corpus uses for `sql`/`git` (`packages/qfs/crates/skill/tests/golden_corpus.rs`);
  covering the parse half first and naming the uncovered remainder is better than a check that
  quietly skips most recipes.

## Final Report

Development completed as planned, with step 2's premise corrected by the evidence.

**Step 1 established which of the two was true, and it was neither.** The ratchet was not lost and
the claim was not invented — the *path* was stale. `git log --all --diff-filter=ADR --name-status
-- '*cookbook_skills.rs'` returns three commits: created at `packages/qfs/crates/test/tests/` by
`f9387de` (repo publish, 2026-07-15), then **renamed to `packages/qfs/xtask/tests/`** by `e17b0a5`
("Typecheck the cookbook ratchet, not just parse it", 2026-08-16) when ratchet 2 started needing the
compiled describe registry. `e17b0a5` is an ancestor of `origin/main`, so the file is present and
enforced on `main` today. (`ef0ae25` shows the same delete, but it is on
`origin/claude/existing-prs-issues-naiqyz` only, not on `main` — a red herring.) The path `CLAUDE.md`
gave, `crates/qfs/tests/cookbook_skills.rs`, has **never** existed at any commit.

**So step 2's "restore it at the path `CLAUDE.md` names" was not the right discharge, and was not
done.** Moving the test to `crates/qfs/tests/` would break it: its own doc comment records that it
may live neither in `qfs-test` (a wasm-clean harness that must not link the binary) nor in the binary
crate (whose dep guard pins it off `qfs-parser`), because ratchet 2 needs both `qfs_parser` and
`qfs::describe::compiled_describe_registry`. `xtask` is the one crate allowed to hold both. The
references were therefore re-pointed at the real path instead — which is what acceptance criterion 3
asks for in as many words ("`CLAUDE.md`'s description of the ratchet resolves to a real file, as does
the comment in `faq_cli_surface.rs`"). Recorded as a decision rather than asked.

**Step 4 needed no work: the check was already where CI runs it.** `xtask` is a workspace member
(`packages/qfs/Cargo.toml`: `members = ["crates/*", "spikes/*", "xtask"]`), so
`packages/qfs/xtask/tests/cookbook_skills.rs` is picked up by plain `cargo test --workspace`, which
CI's `build-test` job runs. The Consideration's worry — that a check behind `gen-skills --check`
would be enforced nowhere, since no CI job invokes `xtask` — does not apply to an integration test in
`xtask/tests/`. The distinction is now written into `CLAUDE.md` so the next reader does not have to
re-derive it.

### Changes

- `CLAUDE.md` — the ratchet's path corrected to `packages/qfs/xtask/tests/cookbook_skills.rs`, with
  one clause on why it lives in `xtask`; a statement that it runs under `cargo test --workspace` and
  is therefore CI-enforced, unlike `gen-docs --check` / `gen-skills --check` / `check-migrations`,
  which no CI job invokes; and a new **What the ratchet does not cover** paragraph.
- `packages/qfs/xtask/tests/cookbook_skills.rs` — a `# What this does NOT cover` section added to the
  module doc comment (acceptance criterion 4), naming the five real limits.
- `packages/qfs/crates/cmd/tests/faq_cli_surface.rs` — its "Home:" paragraph said `cookbook_skills.rs`
  lives in `crates/test/`; corrected, with the move and its date.
- `docs/documentation-map.md` — two entries asserted the file does not exist anywhere. Both corrected;
  gap 7 marked resolved with what was actually wrong.

### Verification

- `cargo test -p xtask --test cookbook_skills` — green: 2 passed. With `--nocapture`: **184 recipes
  extracted, 62 typechecked, 19 sources skipped and printed by path**.
- **Negative probe 1 (parse):** appended `/github/qmu/qfs/pulls |> WHERE ||| state = 'open'` to
  `docs/cookbook/github.md` → `every_cookbook_skill_recipe_parses` FAILED, "1 of 184 cookbook-skill
  recipes do NOT parse … `[UNEXPECTED_TOKEN] at byte 36`". Reverted byte-identically.
- **Negative probe 2 (column):** appended `/github/acme/web/pulls |> where nonexistent_column_probe
  == 'open' |> select number, title` → `every_cookbook_skill_recipe_names_columns_that_exist` FAILED,
  "names [\"nonexistent_column_probe\"] — not columns of /github/acme/web/pulls". Reverted
  byte-identically (`git status --porcelain docs/cookbook/github.md` empty).
- `rg -n 'cookbook_skills' .` — 7 hits, every one resolving: 4 name the real path, 3 name the bare
  filename.
- Gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check`,
  `cargo run -p xtask -- gen-skills --check` — **all exit 0**.

### Discovered Insights

- **Insight**: A "this file does not exist" finding needs `git log --diff-filter=ADR --name-status`
  over the basename, not a path lookup. The documentation-surveying ticket that provoked this one
  checked the two paths the repository *names* (`crates/qfs/tests/`, `crates/test/tests/`), found
  nothing at either, and concluded the ratchet was absent — while it sat at a third path, fully
  implemented, with both promised checks and a floor. The stale reference and the missing artifact
  look identical from the reader's side and are opposite defects.
  **Context**: This repository moves test homes for hard structural reasons (dep-graph guards,
  wasm-cleanliness), so a test's path is comparatively volatile while its filename is stable. Search
  by filename across history before concluding anything is gone.
- **Insight**: `cargo test --workspace` covers `xtask/tests/`, so `xtask` is a legitimate home for an
  anti-drift test that must link the binary — and such a test is CI-enforced even though no CI job
  runs `cargo run -p xtask`. The `xtask` *subcommands* (`gen-docs --check`, `gen-skills --check`,
  `check-migrations`) are the unenforced surface, not the crate.
  **Context**: The two are easy to conflate, and conflating them once already produced a false
  "nothing defends this" conclusion in `docs/documentation-map.md`.
