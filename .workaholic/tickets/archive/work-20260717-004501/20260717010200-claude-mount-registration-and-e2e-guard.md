---
created_at: 2026-07-17T01:02:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash: 9954c77babb8edf7c26680dedc55bf38446c0db1
category: Changed
depends_on: [20260717010100-claude-real-store-reader.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Mount the /claude read path and turn the wrong-reason e2e test into a guard

## Overview

Mission acceptance item 2. `/claude/sessions` raises `unknown_source` because the driver's
introspective mount is **never registered**: `/local`, `/sql`, `/git`, `/sys`, `/transform`,
`/type` each get an `engine.mounts.register(...)` in `packages/qfs/crates/qfs/src/shell.rs`
(`register_cloud_and_sys_mounts`), while `/claude` gets only the conditional read facet
(`shell.rs:328-334`) and no mount — so the planner (`pushdown/src/planner.rs:151` via
`core/src/plan.rs` `resolve_path`) never routes the path. A one-line omission.

Fix: register `Arc::new(qfs_driver_claude::ClaudeDriver::new())` in
`register_cloud_and_sys_mounts` alongside `/sys` — unconditionally, like `/sys` and
`/transform`: describe is pure and credential-free, and a scan with no configured source then
fails closed with the read-registry's structured `unknown_source` ("no read driver registered
for source `claude`", `exec/src/exec.rs:75-82`) instead of the planner's. Correct the t100040
comment in the same function that lists `claude` among CONNECT-gated third-party drivers — it is
a local, credential-free facade like `/sys`, not a cloud surface.

Test correction: `crates/cmd/tests/e2e_cli.rs:356-367`
(`unknown_source_is_capability_exit_three`) today asserts `/claude` → `unknown_source` as
intended behaviour, pinning the bug — it passes for the wrong reason and is why nothing caught
the omission. Rework:

1. keep a genuine planner-`unknown_source` case over a path with no mount at all
   (e.g. `/no-such-mount/x`);
2. `/claude/sessions` with no `QFS_CLAUDE_SESSIONS` stays exit 3 / capability, with the comment
   telling the truth (read facet unwired, mount resolved);
3. **the regression guard**: a new e2e spawning the real binary with `QFS_CLAUDE_SESSIONS`
   pointed at a tempdir FIXTURE store (a `sessions/<pid>.json` carrying the test process's own
   live pid + a matching `projects/<slug>/<uuid>.jsonl`) asserts rows come back with a non-empty
   `last_message`. Remove the mount registration and this test fails with `unknown_source` —
   exactly the guard the mission demands.

## Policies

- `workaholic:implementation` / test — the guard is black-box (spawns the real binary), hermetic
  (fixture store in a tempdir, never the developer's real `~/.claude`), and fails when the mount
  regresses.
- `workaholic:implementation` / coding-standards — comments state the actual gating (mount
  always; read facet opt-in), never an aspiration.

## Quality Gate

1. Both-directions: the new fixture-store e2e written against the unfixed shell **fails** with
   `unknown_source`; after the one-line registration it passes with real rows.
2. `qfs run -e '/claude/sessions |> LIMIT 1' --json` with no env stays a structured exit-3
   capability error (fail-closed unchanged from the operator's view).
3. `DESCRIBE /claude/sessions` keeps working (the describe registry path is untouched).
4. Baseline gates: workspace tests, clippy `-D warnings`, fmt, gen-docs/gen-skills `--check`.

## Considerations

- Registering the mount changes `/claude`'s error from planner-`unknown_source` to
  read-registry-`unknown_source`; both are kind `capability` / exit 3, so agent-facing contracts
  hold. If the read-registry miss should carry an actionable "set QFS_CLAUDE_SESSIONS" hint (the
  t5/t6 connect-hint precedent), do it here — one honest message, not a new error kind.
- The interactive shell and one-shot `run` share `register_cloud_and_sys_mounts`, so both gain
  the mount with the one registration; `qfs serve` wires separately (gate-endpoint ticket).
