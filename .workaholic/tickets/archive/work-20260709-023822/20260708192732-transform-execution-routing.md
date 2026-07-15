---
created_at: 2026-07-08T19:27:32+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: c11dfcd
category: Added
depends_on: [20260708192730-transform-definition-ddl-storage.md, 20260708192731-transform-plan-spine.md]
mission:
---

# Execute transform: provider seam, whole-tree routing, and the irreversible gate

## Outcome (2026-07-09 night drive)

Fully implemented and hermetic. Structure:
- **`ModelProvider` seam** (`driver-transform/src/provider.rs`): the injected model-call trait +
  `ModelRequest` (non-secret selectors + resolved secret out-of-band of the `Debug` request) +
  `UnconfiguredProvider` (the fail-closed default the binary wires until T4's live provider).
  Copied the `SessionSource` shape (pure driver + injected impure backend + mock).
- **`TransformExecutor` seam** (`engine/src/combine.rs`): the engine stays pure — the model call
  is an injected executor the COMMIT boundary provides. `MiniEvaluator::with_transform` runs the
  `CombineOp::Transform` arm; `MiniEvaluator::new` (read/preview) fails closed
  (`transform_no_executor`). The engine enforces declared-OUTPUT membership over the untrusted
  returned rows and reorders to the declared column order. Replaced the single
  `EngineError::TransformNotExecutable` refusal with `TransformNoExecutor`/`TransformFailed`/
  `TransformOutputMismatch`.
- **Binary executor** (`qfs/src/transform.rs::BinaryTransformExecutor`): holds the `ModelProvider`
  + full defs (with secret_ref); resolves `env:`/`vault:` lazily at the call (never logged),
  chunks by mode (row-wise/extraction = per-row calls, relation-wise = one call), projects to the
  declared INPUT. `run_context()` wires it fail-closed with `UnconfiguredProvider`.
- **Whole-tree classifier** (`exec/src/lib.rs::contains_transform` + `eval.rs::collect_transform_
  names`): a `|> transform` ANYWHERE (mid-pipe, subquery source, JOIN source, set-op branch, LET
  binding/body, effect-body pipeline) routes the statement through PREVIEW/COMMIT.
- **Consent/audit + gate**: `eval()` emits one irreversible `CALL transform.<name>` node per stage
  carrying the spend-legibility row (provider/model/effort/mode, NO secret). PREVIEW builds this
  with **zero** provider calls; COMMIT without `--commit-irreversible` is held by
  `IrreversibleGuard`; a TRANSACTION with a transform is rejected (model call is irreversible).
- **Committed-read envelope (§14)**: reused `RowSet.meta.affected` (non-null = effects ran) rather
  than a new renderer method — a committed transform read renders rows + `meta.affected`.
- **Ledger**: `TransformBackend::record_run` (audit RUN event, metadata-only); the applier's
  `CALL transform.<name>` arm ledgers it with the orchestrator-refined exact count.

Acceptance proven hermetically: PREVIEW-zero-calls (every position), no-ack rejection, three
modes, OUTPUT membership violation, whole-tree routing (subquery), committed-read envelope,
env-secret resolution + fail-closed on missing/vault-without-resolver, dep_direction green (the
provider never enters the pure engine; `EffectOutput` unchanged). Full gate green (qfs 319 tests,
exec/engine/core/driver-transform, clippy, fmt, gen-docs/gen-skills/check-migrations). The live
provider run is T4.

## Overview

Third of four dependency-ordered transform tickets (supersedes the deleted mega-ticket
`20260708002200`; design: archived brief `20260708002100` + blueprint §15, Decision W — including
the amended routing ruling at `docs/blueprint.md:686`). With the definition (T1) and the plan spine
(T2) landed, this ticket makes `transform` **run**: the `ModelProvider` seam and injected async
applier, the whole-tree statement classifier that routes any transform-bearing statement through
PREVIEW/COMMIT, the exec-layer orchestration, the three cardinality modes, the irreversible gate
with a model-free PREVIEW, and the committed-read rows + `meta.affected` envelope (§14).

**Discovery corrections (HEAD 24c2269):**
- There is **no `ModelProvider` trait anywhere** — `driver-claude`'s `SessionSource`
  (`crates/driver-claude/src/backend.rs:23-54`) is the structural analogue: a pure driver + an
  injected impure backend + hermetic mocks (`FakeSource` applier.rs:120, `FixtureSource`
  lib.rs:233). Copy that shape; this ticket introduces the trait.
- The existing classifier is **terminal-only**: `crates/exec/src/lib.rs:206-220` reclassifies a
  pipeline whose *last* op is `CALL`. The §15 ruling requires an **anywhere-in-tree** walk
  (mid-pipe, subquery, JOIN source, set-op branch, LET binding/body).
- There is **no combined rows+affected envelope today** — `Renderer` has separate `rows()`/`plan()`
  (`crates/exec/src/output.rs:31,37`); the committed-read envelope is a new seam.

The model call is **exec-layer orchestration** at the commit boundary: upstream read → injected
applier (holds the `ModelProvider`) → OUTPUT membership check → downstream segment or write `args`.
The effect node is the consent/audit artifact; row payloads flow exec-side, above the interpreter —
`EffectOutput` stays `{id, affected}` and the pure engine stays pure.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the trait + applier live in the
  binary-side composition (the `driver-claude` template), never in the pure/wasm engine.
- `workaholic:implementation` / `policies/coding-standards.md` — typed provider config; no stringly
  model plumbing; secrets only as references resolved at COMMIT.
- `workaholic:implementation` / `policies/domain-layer-separation.md` — pure plan/interpreter vs.
  impure injected applier; the CO-t29-4 dependency-direction guard must stay green.
- `workaholic:implementation` / `policies/test.md` — hermetic mock-provider execution tests across
  the whole spine; the workspace stays network/credential-free.
- `workaholic:design` — safety: PREVIEW calls **no model** (spend legibility before consent);
  COMMIT of a transform requires the explicit irreversible ack; failures are structured and
  secret-free.

## Key Files

Verified anchors at HEAD `24c2269` (2026-07-08):

- `packages/qfs/crates/exec/src/lib.rs:198,206-220` — the terminal-`CALL` reclassification to
  generalize into the whole-tree classifier (any `PipeOp::Transform` anywhere → PREVIEW/COMMIT).
- `packages/qfs/crates/exec/src/lib.rs:288,328` — `preview_or_commit` + commit-boundary
  materialization (`materialize_pipeline_source`, cap `MAX_MATERIALIZED_ROWS` `:315`): the
  orchestration point (read → applier → membership check → downstream/write args).
- `packages/qfs/crates/exec/src/output.rs:26,31,37` — `Renderer` (`rows()` vs `plan()`): add the
  committed-read envelope rendering **rows + `meta.affected`** (§14), never just a commit summary.
- `packages/qfs/crates/core/src/security.rs` — `IrreversibleGuard::decide` (used at exec
  `lib.rs:273`): the COMMIT gate to ride.
- `packages/qfs/crates/core/src/eval.rs:787` — where `write_irreversible` is consulted while
  building the effect plan; the transform effect node sets its irreversible bit here-adjacent.
- `packages/qfs/crates/driver-claude/src/backend.rs:23-54` — `SessionSource`: the injected-impure-
  backend template for the new `ModelProvider` trait.
- `packages/qfs/crates/driver-claude/src/applier.rs:28,35,82,91,120` — `ClaudeApplier` (holds
  `Arc<dyn SessionSource>`) + `FakeSource`: the applier + mock shape to copy.
- `packages/qfs/crates/qfs/src/claude.rs:53,60,68` — the composition-root injection pattern
  (`DirSessionSource::open_default`): where the live provider binds in the binary.
- `packages/qfs/crates/cmd/tests/dep_direction.rs` — the dependency-direction guard that must keep
  the provider out of the pure engine.

## Related History

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) — settled: injected applier, irreversible, model-free PREVIEW, §15 routing.
- `docs/blueprint.md:686` — the amended routing ruling (whole-tree classifier, exec-layer
  orchestration, committed-read rows+affected, server-body rejection — the rejection itself landed
  with T1).
- t64 `/claude` driver — the pure-driver + injected-applier contract reused (crate in tree).

## Implementation Steps

1. **`ModelProvider` seam:** define the trait (async call taking the definition's model/effort +
   input rows, returning output rows or a structured error) next to a deterministic mock (the
   `FakeSource` pattern). The binary crate owns the live impl; the engine never sees it.
2. **Injected applier:** a transform applier holding `Arc<dyn ModelProvider>`; registered from the
   composition root (the `claude.rs` pattern). Secret resolution (vault/env reference from the T1
   definition) happens here at COMMIT — lazily, never logged, never at PREVIEW.
3. **Whole-tree classifier:** generalize `exec/lib.rs:206` from terminal-`CALL` to a full-tree walk —
   a `transform` in mid-pipe, subquery, `JOIN` source, set-op branch, or `LET` binding/body routes
   the statement through PREVIEW/COMMIT, never the direct read executor.
4. **Orchestration:** at the commit boundary, run upstream read → applier (per the mode: row-wise /
   relation-wise / extraction, driven by the definition's derived mode) → OUTPUT schema membership
   check → feed the downstream segment or the terminal write's `args` (composing with
   `materialize_pipeline_source`). `EffectOutput` stays `{id, affected}`.
5. **Safety gate:** the transform effect is irreversible (spends tokens, non-deterministic):
   PREVIEW builds the effect-plan showing estimated model/effort/row count **without any provider
   call**; COMMIT without the irreversible ack is rejected via `IrreversibleGuard::decide`.
6. **Committed-read envelope:** a committed statement whose terminal is a read renders **rows +
   `meta.affected`** through a new `Renderer` seam (§14) — never just the plan summary.
7. **Failure modes:** missing/invalid provider credential fails closed at COMMIT with a structured,
   secret-free error; provider errors surface as terminal effect errors (no partial silent rows).
8. Delete T2's temporary exec-layer refusal; the spine now executes end to end against the mock.

## Quality Gate

Distributed from the parent mega-ticket's gate (owner-approved 2026-07-08); plus the common gate.
The live-provider run is **NOT** here — it gates the final ticket (`20260708192733`).

**Acceptance criteria:**

- **PREVIEW calls no model:** the mock provider records **zero** invocations across a PREVIEW of
  every mode (asserted).
- COMMIT without the irreversible ack is rejected; with the ack, the mock-backed run executes and
  the ledger records the effect.
- **Routing:** a `transform` in each nested position — mid-pipe, subquery, `JOIN` source, set-op
  branch, `LET` binding and body — classifies the statement as effect-bearing and routes to
  PREVIEW/COMMIT (one test per position), never the direct read executor.
- A committed read renders **rows + `meta.affected`** (§14 envelope test).
- A `transform` feeding a terminal write composes with commit-boundary materialization: the write's
  `args` carry the OUTPUT rows (test).
- All **three modes** execute against the injected mock with deterministic fixtures; OUTPUT
  membership violations are structured errors.
- Missing/invalid credential fails closed at COMMIT; no secret in any output or error (asserted).
- `dep_direction` guard green: the provider never enters the pure engine; `EffectOutput` unchanged.

**Verification method:**

- Hermetic: `cargo test -p qfs-exec -p qfs-core -p qfs-runtime -p qfs -p qfs-cmd` (workspace when
  disk allows) with the injected mock provider; `clippy --workspace --all-targets -D warnings`;
  `fmt --all --check`; `gen-docs --check`.

**Gate:** all green; zero network/credentials in the suite; PREVIEW-zero-calls and
no-ack-rejection proven against the mock.

## Considerations

- Depends on both `20260708192730` (definition + secret reference + mode function) and
  `20260708192731` (plan spine) — the orchestration consumes the plan's mode + OUTPUT schema.
- The docs/version ticket (`20260708192733`) documents this surface and runs the single live
  provider check — keep this ticket fully hermetic.
- This dev host has LIVE cloud accounts connected; never verify against a real provider here
  (`.claude` memory: qfs-env-has-live-cloud-accounts) — the live run is T4's explicitly-approved,
  recorded, out-of-band step.
- The tunnel between preview purity and spend legibility was settled in the brief: the PREVIEW
  effect-plan carries estimates only (definition metadata), no provider call — mirror how Drive's
  create-only guard kept preview pure (commit `4f3fa50` discussion).
