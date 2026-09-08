---
created_at: 2026-09-07T18:34:04+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
merge_policy: review
---

# Verify Codex plugin distribution and prevent shared-skill drift

## Overview

Make the existing QFS plugin's Codex installation path documented and verifiable, and continuously check the shared distribution that Claude Code and Codex consume. This is maintenance of an existing compatible package, not a port or a second set of skills.

The 2026-09-07 audit at commit `277504f` found both host manifests and `.agents/plugins/marketplace.json` already present. All four existing plugin version fields equal `0.22.1`; 13 generated skills match their Cookbook sources byte-for-byte, and all 14 skill directories match the Claude marketplace list. The base `qfs` skill is separately authored, outside the Cookbook generator. `gen-skills --check` currently checks generated bodies, Claude symlinks, and substring membership in the Claude marketplace, but not Codex paths, exact skill membership, or version consistency. CI does not invoke that check. These are coverage gaps, not evidence that the present versions differ.

`docs/guide/installation.md` only teaches Claude Code installation. Codex CLI `0.153.4` was available, but `codex plugin marketplace list` did not include QFS and `codex plugin list --available --json --marketplace qfs` returned empty lists. Actual installation and loading were not tested. Cargo was unavailable; the audit compared files with the generator's rendering rules instead of claiming a Rust test pass.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — extend the existing xtask, plugin, and documentation locations.
- `workaholic:implementation` / `policies/coding-standards.md` — preserve typed validation and the repository's Rust conventions; TypeScript-specific syntax rules do not apply to Rust.
- `workaholic:implementation` / `policies/objective-documentation.md` — distinguish package structure, discoverability, installation, and verified loading.
- `workaholic:implementation` / `policies/command-scripts.md` — one local checker, also invoked by CI.
- `workaholic:implementation` / `policies/test.md` — prove missing or inconsistent distribution artifacts are actually rejected.
- `workaholic:operation` / `policies/ci-cd.md` — local checks remain reproducible; existing hosted CI is a backstop.

## Key Files

- `packages/qfs/xtask/src/gen_skills.rs` — generation and current drift checking.
- `packages/qfs/xtask/src/main.rs` — checker invocation and error reporting.
- `packages/qfs/xtask/tests/cookbook_skills.rs` — existing recipe validation and dependency boundary.
- `plugins/qfs/.codex-plugin/plugin.json`, `plugins/qfs/.claude-plugin/plugin.json` — shared package identity and version.
- `.agents/plugins/marketplace.json`, `.claude-plugin/marketplace.json` — host-specific distribution paths and registrations.
- `plugins/qfs/skills/qfs/SKILL.md` — separately authored base skill; cover existence and registration without inventing a Cookbook origin.
- `.github/workflows/ci.yml` — invoke the canonical checker from the existing Rust workspace.
- `docs/guide/installation.md`, `docs/guide/repository.md`, `CLAUDE.md` — installation instructions and accurate coverage descriptions.

## Related History

The FAQ skill already established directory discovery for Codex. Earlier investigation also corrected the mistaken claim that the Cookbook recipe ratchet did not exist: it lives in xtask and is distinct from generated-output equality.

- `.workaholic/tickets/archive/work-20260706-175903/20260706163521-qfs-faq-reference-skill.md` — FAQ distribution and shared plugin version policy.
- `.workaholic/tickets/archive/work-20260817-124626/20260817105331-the-cookbook-recipe-ratchet-two-places-name-does-not-exist.md` — actual location and limits of existing checks.
- `.workaholic/tickets/archive/work-20260704-181053/20260703150400-plugin-cache-staleness.md` — skill changes need a version bump to propagate through caches.

## Implementation Steps

1. Re-establish the baseline on the implementation checkout: enumerate generated versus separately authored skills, resolve both marketplaces and host manifests, and run the existing checker. Record host versions and current loading behavior separately from static compatibility.
2. Extend the canonical local check to parse distribution JSON structurally, validate the QFS source and skills paths, compare exact skill registration to the directories present including the base skill, and compare the four existing version fields. Keep host-specific metadata differences legal. The Codex marketplace has no version field and must not acquire one just to satisfy the check.
3. Add negative fixtures for a changed generated body, missing base skill, missing/extra registration, malformed manifest, incorrect Codex source/skills path, and mismatched version. Assert useful path-specific failures rather than mirroring implementation internals. Cover the canonical base-skill symlink where repo-local Claude discovery depends on it.
4. Invoke the same check in branch/PR CI. Update coverage statements in CLAUDE.md and the repository guide to reflect precisely what now runs and what remains outside the check.
5. Add Codex installation and verification instructions alongside Claude Code, based on current official documentation and the available CLI help. Explain the separate QFS binary and service setup, shared skill content, and how users confirm the plugin is loaded. Follow those instructions in an isolated plugin/config environment and verify the actual loaded skill set; mere JSON parsing is not proof of loading.
6. Run the quality gate. Apply the repository's versioning rules to the implementation PR: binary patch per shipped PR, and all four plugin fields together if skill-affecting content changes. Regenerate affected generated artifacts from source.

## Quality Gate

**Acceptance criteria**

- A Codex user can follow the published instructions to discover, install, and confirm loading of the existing QFS plugin, with the QFS binary prerequisite clearly identified.
- Claude Code and Codex still consume one shared skill tree; every actual skill, including the separately authored base skill, is accounted for.
- The canonical check succeeds on the correct tree and fails with the affected path for each negative fixture listed above.
- Branch/PR CI invokes that same locally runnable checker; docs describe the actual coverage without claiming execution-semantic guarantees from byte equality.

**Verification method**

- Decided: use isolated host installation/loading smoke verification plus hermetic checker fixtures — these prove distribution and discovery without requiring mail, Slack, or other service credentials (developer may override at /drive).
- Record the tested Codex version, exact installation commands, discovered version and loaded skill names. If loading cannot be tested, report that as unverified and do not substitute a static manifest check.
- Run `cargo run -p xtask -- gen-skills --check`, targeted xtask tests, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, and `cargo run -p xtask -- gen-docs --check` from `packages/qfs`; run `npm run docs:build` at root.

**Gate**

- Checker positive/negative cases, recorded Codex loading smoke, relevant Rust checks, and docs build pass before the implementation is reported complete.

## Considerations

- Content equality does not prove that instructions teach correct runtime behavior. The companion `20260907183405-align-preview-guidance-with-mount-routing.md` addresses the measured preview mismatch independently.
- A separately authored base skill is not inherently wrong. Do not duplicate it per agent or force it into a new generation scheme merely to make counts uniform.
- Existing active missions were inspected; these maintenance tickets are kept standalone rather than expanding an existing mission's completed acceptance scope. `review` records the default merge policy and does not authorize merging this ticket publication PR.
- Official packaging reference checked during the audit: https://developers.openai.com/plugins/build/plugins . Re-check installation syntax at implementation time.

## Final Report

Development completed as planned. The shared QFS plugin now has one structural distribution check,
`scripts/check-plugin.py`, covering both host manifests, both marketplaces, synchronized versions,
exact skill registration, the base-skill symlink, and byte-for-byte Cookbook-derived skill content.
`scripts/test-check-plugin.py` supplies 11 hermetic positive and negative cases, including malformed
metadata, missing and extra registrations, incorrect Codex paths, version mismatch, and generated
body drift. CI invokes this checker alongside the existing Rust generator check.

Codex installation was verified with Codex CLI 0.153.4 from the repository marketplace: `qfs@qfs`
installed, listed as enabled, and resolved plugin version 0.22.3 from the shared skill tree. The
installation guide now documents the supported Codex marketplace flow and keeps the QFS binary and
service connection prerequisites distinct from plugin discovery. The binary patch version is
0.0.131 and all plugin version-bearing metadata is synchronized at 0.22.3.

The local Python checker and its 11 tests passed, as did `npm run docs:build`. GitHub CI supplied
the unavailable local Rust toolchain and passed the workspace build/test, clippy, rustfmt,
generator, serialized library, viewer, wasm, and cross-compilation gates.

### Discovered Insights

- **Insight**: generated-body equality and host installation are separate guarantees. The former
  prevents source drift; the latter proves that Codex accepts the marketplace and exposes the
  package. Both are now checked without creating a second Codex-specific skill tree.
- **Insight**: a small standard-library Python checker is the common executable boundary for the
  two JSON distributions, while the Rust xtask remains the authoritative generator backstop. This
  keeps local and CI checks reproducible even before a Rust toolchain is available.
