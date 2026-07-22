---
created_at: 2026-07-22T09:05:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260722090200-per-row-decode-over-collected-sets.md, 20260722090300-documents-links-as-declared-registrations.md]
mission: a-file-collection-is-a-declared-set-over-any-blob-source
---

# The cookbook stops teaching what the binary rejects

## Overview

Mission acceptance item 7. The cookbook ratchet (`crates/test/tests/cookbook_skills.rs`) is
parse-only, which is how a dozen multi-file `decode` recipes shipped teaching pipelines the
binary rejected. Close that class of divergence on this surface:

1. Sweep `docs/query-cookbook.md` and the cookbook articles (`docs/cookbook/*.md`) for every
   multi-file `decode` / collection recipe.
2. For at least the collection recipes, raise the ratchet from parse-checked to
   **execution-checked**: each recipe runs against a hermetic fixture tree and its result shape
   is asserted — not just its parse.
3. Any recipe that cannot be made true is corrected in the article (and the skills regenerate
   from the articles — never hand-edit a SKILL.md).

Docs-true is restored on this surface: an agent reading the skills can no longer be taught a
statement the binary rejects, for the collection family.

## Policies

- The ratchet only tightens: recipes already execution-checked stay execution-checked; do not
  downgrade any check to make a recipe pass.
- Fix the binary or fix the article — never weaken the assertion to split the difference.
- Hermetic fixtures only.

## Quality Gate

- The collection recipes in the cookbook are execution-checked in the test suite; reverting
  the per-row decode makes them fail.
- `cargo test --workspace`, clippy `-D warnings`, `cargo fmt --all --check` pass.
- `gen-skills --check` passes after any article correction (skills regenerated, not edited).
