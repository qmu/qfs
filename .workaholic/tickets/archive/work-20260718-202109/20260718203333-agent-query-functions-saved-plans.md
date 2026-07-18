---
created_at: 2026-07-18T20:33:33+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260718203331-create-agent-grammar-registry.md, 20260718203332-agent-subject-policy-gate.md]
mission: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
---

# Agent query functions: named saved plans declared as data, invoked through preview-by-default

## Overview

Give an agent named query functions as saved plan rows — the JobDecl `DO <plan>` body shape
WITHOUT a cadence — under its `/server/agents` surface, readable exactly like `/server/jobs`
rows.

Concrete work:

- Store a function as a named plan row on the agent's registry surface (no cadence field).
- Invocation (`qfs agent run <agent> <fn>` or equivalent) builds via `qfs_exec::build_plan`,
  previews by DEFAULT, and commits only through the same policy + IrreversibleGuard chain as the
  sweeper's `LiveCronCommitter` — evaluated under the AGENT's `DecisionContext`, not the
  operator's.
- The §5.9 pure-lambda effects ban STAYS intact: a function is a named gated statement, not a
  lambda; no new execution semantics — it desugars to the shipped preview/commit pipeline.

## Policies

- Preview-by-default: a builtin can never shortcut the gate; invocation without `--commit` produces zero effects.
- §5.9 pure-lambda effects ban untouched: a function is a gated statement, not a lambda.
- Secret-free reads: reading an agent's function surface lists them credential-free.

## Quality Gate

1. Function declarations round-trip as registry rows (reading the agent's surface lists them credential-free).
2. Invoking WITHOUT `--commit` previews (zero effects); WITH commit, effects pass the agent-subject gate.
3. A function touching an ungranted path is denied with the agent named in the reason.
4. No new execution semantics — the function desugars to the shipped preview/commit pipeline.
5. The §5.9 pure-lambda effects ban is untouched.
6. Verification: `cargo test -p qfs-cmd -p qfs-qfs` — hermetic e2e over a temp registry, no live services.

## Considerations

- Reuse `build_plan` and the existing preview/commit pipeline; do not add a parallel execution path for agent functions.
- The commit path must evaluate under `DecisionContext::for_agent` from 203332 — an agent function commit is gated by the agent's grants, never the invoking operator's.
- A function carries no cadence; scheduled launch of a function is the sweeper ticket (203334).
