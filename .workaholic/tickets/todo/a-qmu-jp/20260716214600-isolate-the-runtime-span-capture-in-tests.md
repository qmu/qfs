---
created_at: 2026-07-16T21:46:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission:
---

# Isolate the runtime span capture so parallel tests stop flaking

## Overview

Concern `qfs-runtime-span-buffer-test-flakes`. Verified against source this session: the flake
is test-harness state, not runtime code.

- `packages/qfs/crates/runtime/tests/txn_commit.rs:553-601` installs a `Capture` tracing
  subscriber **process-globally, once** (`static CAPTURE: OnceLock`, `set_global_default`
  inside `get_or_init`), with one shared `Mutex<Vec<String>>` line buffer.
- The consumer `observability_spans_carry_ids_and_are_secret_free` (:610) filters SOME
  assertions to its unique plan id (`plan-obs-unique`, :623-642) but runs others against the
  whole cross-test-polluted buffer: `all.contains("leg applied")`, `all.contains("effect.id")`
  (:644-645) and the secret-free scan over every line (:646+). Under the parallel harness the
  buffer collects spans from every other test's commits (`acid_strategy_clean_commit` :85,
  `mixed_plan_audit_is_ordered_and_secret_free` :469, …), so the `all`-scoped checks depend on
  interleaving — the flake surface.
- The process-global `TRACE_SEQ` counter (`runtime/src/observe.rs:29`) is also shared, so any
  ordering assumption across tests is unstable by construction.

## Implementation Steps

1. Replace the global installation with per-test scoping:
   `tracing::subscriber::with_default(cap, || …)` around the commit under test, giving the test
   its own buffer — the comment at :589-591 says the global default existed to raise tracing's
   max level; keep that via a cheap global no-op subscriber or `LevelFilter` handle if needed,
   while the capturing subscriber stays scoped.
2. Scope every remaining assertion to the test's plan id (the way `mine` already is), so even a
   future shared line can never leak into an assertion.
3. Drop any reliance on process-monotonic `TRACE_SEQ` ordering across tests (assert per-plan
   ordering only).

## Key Files

- `packages/qfs/crates/runtime/tests/txn_commit.rs:553-660` — the capture and its consumer.
- `packages/qfs/crates/runtime/src/observe.rs:29-40` — the trace-id mint (context only; likely
  unchanged).

## Policies

- `workaholic:implementation` / `test` — hermetic includes hermetic FROM SIBLING TESTS; shared
  mutable harness state is the same defect class as a shared config home.

## Quality Gate

1. `cargo test -p qfs-runtime` green repeatedly under the parallel harness AND under
   `--test-threads=1` (both orders of magnitude: run the suite in a loop locally ~20x with no
   flake).
2. The secret-free scan still runs — scoped to the test's own emissions — and still fails when
   a secret-bearing line is planted (both-directions: plant one, watch it fail).
3. Baseline gates + patch bump.

## Considerations

- Keep the fix inside the test harness; `qfs-runtime`'s emit path is not the defect.
- If `with_default` misses spans emitted from spawned threads inside the interpreter, scope via
  a subscriber that tags by thread/plan instead — decide in-ticket, record in the commit body.
