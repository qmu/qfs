---
created_at: 2026-07-22T09:01:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: a-file-collection-is-a-declared-set-over-any-blob-source
---

# Design brief: the codec relation surface, and the §13b ruling recorded

## Overview

Mission acceptance item 4. Two deliverables, both documents — no production code in this ticket:

1. **Record the §13b ruling in the blueprint.** Blueprint §13b's open question ("builtin driver,
   or one alias-registration set?") is closed as the mission rules it: **alias registration
   wins** — a collection is a declared, named set registered over other paths, a stored view
   through the existing definition layer with zero new grammar (§3: `CREATE <noun>` desugars to
   an `INSERT` into a registry path). `/markdown` is demoted from builtin to one instance of the
   general shape. Rewrite the §13b text so it states the ruling, not the question.
2. **Rule the codec relation surface in a design brief** (in the mission directory,
   `.workaholic/missions/active/a-file-collection-is-a-declared-set-over-any-blob-source/`).
   The markdown interpretation yields TWO named relations of the same format — the flat
   per-document relation (`decode md` today) and the link relation (the full nested
   `section_path` graph). The brief rules **how the second relation is named and reached**.
   Candidates the mission names (owner delegated the pick to this brief, 2026-07-22):
   a relation-qualified format name, a relation argument to `decode`, or codec-declared named
   outputs. Rule ONE, against the source of `crates/codec` and the planner — with the
   constraints fixed by the mission: both relations exist, each is named, neither is inferred,
   and `decode md` keeps yielding the flat per-document relation unchanged.

The brief must also restate the two contracts the implementation tickets consume, so they are
written down before code exists: per-row decode over a multi-row content-bearing set with
per-file relations unioning (the single-file case is the one-row instance), and provenance —
the root-relative `path` column carried through every decode as the canonical join id, owned by
the decode application, not by each codec.

## Policies

- This is design-then-implement: the implementation tickets in this mission depend on this
  brief and must not start ahead of it.
- qfs is experimental — the rulings are redefinitions, not migrations. No compatibility
  framing, no deprecation periods.
- The ruling must not fork the semantics owned by the sibling DSL mission; where a declared
  driver decodes a collected response set, it rides the same per-row rule ruled here.

## Quality Gate

- Blueprint §13b reads as a closed ruling (alias registration; `/markdown` demoted; zero new
  grammar via §3) with no remaining "open question" phrasing on this point.
- The design brief exists in the mission directory, rules exactly one relation-surface
  spelling with its rejected alternatives and reasons, and states the per-row decode and
  provenance contracts the implementation tickets build against.
- `cargo run -p xtask -- gen-docs --check` still passes (docs untouched by a design-only
  change stay in sync).
