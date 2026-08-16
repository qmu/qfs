---
created_at: 2026-08-16T21:30:14+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260816-213314
---

# A write to a declared driver with no matching `CREATE MAP` previews as if it would work

## Overview

PREVIEW is the safety instrument: an operator (or an agent) reads the preview to learn what a
statement would do before committing it. For a declared driver it answers for a write the driver
**cannot perform at all** — one whose path and verb match no `CREATE MAP` — with an ordinary effect
row, indistinguishable from a write that would succeed.

Measured 2026-08-16 against `qfs 0.0.107` with the shipped `slack_driver.qfs` installed (17 rows),
which declares no `MAP UPSERT` for any `/slack/…` path:

```
$ qfs run "/local/tmp/t.csv |> decode csv |> select 'a.txt' as filename, a as bytes \
    |> upsert into /slack/acme/files"
{"preview":{"rows":[{"id":0,"verb":"READ","target":{"driver":"local","path":"/local/tmp/t.csv"},…},
                    {"id":1,"verb":"UPSERT","target":{"driver":"slack","path":"/slack/acme/files"},
                     "affected":"unknown","irreversible":false}],…},"committed":false}
```

Contrast a **read** of an unrouted path, which refuses at plan time with `unknown_source` / exit 3.
The write side is the asymmetry: nothing between the parser and the applier asks whether a map
exists, so the refusal — if one comes at all — arrives at commit, after the operator has read a
preview that said the write is fine.

This is how `docs/cookbook/slack.md` could teach an `UPSERT INTO /slack/<ws>/files` for months after
the compiled driver that implemented it was deleted (ticket `20260813024753`): every hand-check of
the recipe previewed cleanly. The cookbook ratchet did not catch it either, and
`20260725143000` (typecheck the ratchet) has since raised that half — but the ratchet checks
recipes in the repository, while this is what an operator's own statement does at the terminal.

## Scope

**In scope:** make a declared-driver write whose (path, verb) matches no installed `CREATE MAP`
refuse at **plan time**, with a structured code naming the path and the verb, the way an unrouted
read already refuses.

**Out of scope:**

- Compiled drivers, which resolve their write capability through `Capabilities` at resolve time —
  the gap is specific to the declared path.
- The preview's `affected: unknown` for declared writes; estimating row counts is a separate
  question and not what this ticket is about.
- `describe` on a declared mount, which is its own defect (`20260728085253`).

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — the preview must refuse what the driver
  cannot do rather than render a plausible row for it.
- `workaholic:implementation` / `policies/coding-standards.md`.
- `workaholic:operation` / `observability` — a preview that cannot distinguish "will work" from
  "cannot work" is an instrument reporting something other than the system's state.

## Key Files

- `packages/qfs/crates/exec/src/declared.rs` — `eval_map_body` and the declared write seam; the map
  lookup that would have to run (or be mirrored) at plan time.
- `packages/qfs/crates/core/src/eval.rs` — where a plan-time write refusal is raised
  (`EvalError::DriverWrite` / `UnroutedPath` are the neighbouring shapes and codes).
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — the declaration whose missing
  `MAP UPSERT` this was measured against.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — where a test would sit, beside the existing
  declared-write tests.

## Implementation Steps

1. Reproduce with the command above and record the raw preview, then the raw commit outcome — the
   commit-time behaviour decides whether this is "late refusal" or "no refusal at all", and the
   ticket's wording must match what the binary does.
2. Decide where the check belongs: the evaluator (needs the installed maps at plan time) or the
   declared read/write seam that already loads them.
3. Refuse with a structured, AI-consumable error naming the path and the verb, and listing the verbs
   the mount does declare — the recovery information an agent needs.
4. Test it beside the declared-driver tests: an unmapped `UPSERT` refuses at plan time; a mapped one
   (the post map, the file detach) still previews and commits unchanged.

## Quality Gate

**Acceptance criteria**

- An `UPSERT INTO /slack/<ws>/files` against the shipped declaration refuses at plan time with a
  structured code and a non-zero exit, naming the verbs `/slack/<ws>/files` does declare.
- A mapped declared write (`INSERT INTO /slack/<ws>/<channel>/messages`,
  `REMOVE /slack/<ws>/files/<id>`) previews and commits exactly as before.
- The refusal reaches the operator as prose, like every other plan-time refusal.

**Verification method**

- The command in the Overview, re-run against the built binary with its raw output and `EXIT=` code
  pasted into the ticket outcome.
- The new tests, plus `cargo test --workspace`.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-16 while driving `20260813024753`, whose
  step 1 asked for the gap to be confirmed "from the binary rather than from the source". Confirming
  it is what surfaced this: the probe that was supposed to *fail* for a retired upload previewed
  green.
- Worth checking whether the same asymmetry exists for a declared `CALL` naming no map.
