---
created_at: 2026-07-18T20:33:31+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260718203330-agent-model-blueprint-chapter.md]
mission: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
---

# CREATE AGENT <name> parses on the closed core and desugars to /server/agents registry rows

## Overview

Introduce `CREATE AGENT <name>` as a binding DDL that parses on the closed core and desugars to
`/server/agents` registry rows, exactly as every other server binding does.

Concrete work:

- Add `AGENT` as a **contextual noun** in `parser/src/grammar.rs` (matched by `word()` like
  `TRANSFORM`/`TABLE` at grammar.rs:1349,1374) — the 39-keyword freeze stays intact, so a column
  named `agent` still parses everywhere.
- Add an `AgentDecl` in `core/src/ddl/server.rs` beside `EndpointDecl`/`JobDecl`/`ViewDecl`.
- Wire the `binding_config_row` / `config_row_batch` / `desugar_to_insert` path so `CREATE AGENT`
  lands rows on `/server/agents` like every other binding (same INSERT-plan shape as
  `ServerBindingDdl::Job`).
- `DESCRIBE /server/agents` renders credential-free.
- The §16 provision dump/restore loop round-trips an agent binding.
- `REMOVE` drops an agent binding through the standard gate.

## Policies

- Closed-core grammar: `AGENT` is a contextual identifier; the keyword freeze count is unchanged; no new frozen keyword is added.
- gen-docs anti-drift: regenerate reference docs from the binary, never hand-edit `docs/{language,drivers,server}.md`.
- Experimental, no backward compatibility: no migration path for the new binding shape is required.

## Quality Gate

1. `CREATE AGENT <name>` parses, with `AGENT` matched by `word()` and NOT a new frozen keyword (a column named `agent` still parses everywhere).
2. Desugar produces the same INSERT-plan shape as `ServerBindingDdl::Job` (`server_write_plan`/`desugar_to_insert`).
3. `REMOVE` drops an agent binding through the standard gate.
4. `qfs plan`/`apply` round-trips an agent binding.
5. `DESCRIBE /server/agents` output contains no secret material.
6. An e2e test in the style of `cmd/tests/e2e_binding_ddl.rs` covers create → describe → remove.
7. The keyword-freeze test is unchanged in count; `dep_direction.rs` still passes.
8. Verification: `cargo test -p qfs-parser -p qfs-core -p qfs-cmd`; `cargo run -p xtask -- gen-docs --check`.

## Considerations

- Follow the blueprint chapter (20260718203330) for the exact registry shape and the row-home ruling; do not invent a `/sys` identity here.
- The agent binding carries no cadence and no plan body yet — this ticket is the naming + registry landing only; functions (203333) and cadence (203334) build on the row.
- Keep `DESCRIBE` credential-free from the start; the secret posture is policy-subject only (blueprint §secret posture).
