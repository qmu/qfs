---
created_at: 2026-07-11T12:15:32+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 4h
commit_hash:
category: Added
depends_on: [20260711121529-live-model-providers-anthropic-openai-google.md]
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Switch predicate in the pipe: let the model choose the branch/tool

> **STATUS (2026-07-12, third update): the OWNER-ATTENDED LIVE ROUTING ROUND RAN and PASSED —
> the ticket's gate is closed.** Statement (scratchpad `switch-live-round.qfs`): `/mail/inbox |>
> select subject |> limit 3 |> extend instruction = … |> transform triage |> switch route {
> 'file' => … insert into /drive/my/qfs-switch-test, else => … insert into /mail/drafts }` with
> `triage` = anthropic / claude-haiku-4-5-20251001 / effort low / `env:ANTHROPIC_API_KEY`.
> Result: `COMMITTED — #0 CALL transform.triage [affected 3] (!) / #2 INSERT drive [affected 2]
> / #4 INSERT mail drafts [affected 1]` — the real model ran ONCE, routed 2 alert-looking
> subjects to the Drive arm and 1 to the drafts arm; the routed Drive file's content was the
> subject text (text→bytes coercion works). The round surfaced FOUR real defects, each now fixed
> or ticketed:
>
> 1. **Live-transform commit panicked** ("cannot start a runtime from within a runtime",
>    `driver-http/client.rs`): the sync HTTP client `block_on`s inside the exec read runtime —
>    latent in EVERY live `|> transform` commit (hermetic mocks never do HTTP). FIXED: ambient-
>    runtime detection drives the owned runtime on a worker thread + loopback regression test.
> 2. **Switch prune dropped dependency edges instead of bridging** (my regression): the else arm
>    became dep-free and fired BEFORE the Drive arm's failure aborted the plan — breaking §18-C
>    declaration order and fail-stop (observed live as a stray draft from the failed attempt).
>    FIXED: `prune_nodes_bridging` (every parent → every child) + unit and e2e dep-chain tests.
> 3. **Drive folder INSERT loses rows past the first** (affected 2, one file written) + missing
>    destination folder fails at COMMIT not PREVIEW (cookbook promises preview) — ticketed:
>    `20260712005000-drive-multi-row-insert-silent-loss.md`.
> 4. **Chatwork declared driver live read returns rows with zero columns** — ticketed:
>    `20260712005100-chatwork-declared-live-read-empty-columns.md`.
>
> Residue left in the owner's accounts (cleanup optional, owner's call): Drive folder
> `qfs-switch-test` with 1 routed file, 2 self-addressed drafts (`Routed by the qfs switch live
> round.`), and the `triage` transform definition (`remove transform triage` drops it).
>
> **STATUS (2026-07-11, second update): Steps 2–5 DONE — implemented (v0.0.56, plugin 0.11.0).**
> Owner directed implementation on this branch (drive session). Shipped: `PipeOp::Switch` AST +
> grammar (`switch`/`else` contextual idents, a bounded arm-boundary token scan so greedy
> projection commas never swallow the next arm), variant-count lock 19→20 (keyword freeze
> unmoved at 39), eval-side arm-union planning (each write arm = the `INSERT … FROM` plan shape
> via `eval_write`, each CALL arm = the terminal-call shape via `eval_terminal_call`, relabelled
> and sequenced in declaration order), resolve-stage arm gates (write capability + CALL
> procedure), and the exec commit boundary: one source materialization (the model runs once) →
> partition by discriminant → per-arm continuation folded via `MiniEvaluator` over a synthetic
> `(values)` leaf → rows embedded into the consented write node → untaken arms pruned
> (previewed-but-not-fired) → one apply. Hermetic suites: 7 parser + 10 eval + 5 e2e tests;
> docs (blueprint §18 status + scope cuts, language reference EBNF, cross-service cookbook
> recipe "let the model pick the tool"), gen-docs/gen-skills clean.
>
> **First-slice scope cuts (recorded in blueprint §18):** `else` mandatory for every switch
> (closed-enum label coverage awaits refinement-carrying schemas), all-pure switch deferred
> (arms must be effect-terminal), arm vocabulary row-local (no join/set-ops/codecs/transform/
> nested switch), UPDATE/REMOVE not arm terminals, committed summary lists fired arms only.
> The owner-attended live routing round (real model choosing between two real arms) remains,
> per the Quality Gate below.
>
> **STATUS (2026-07-11): Step 1 (design first) DONE.** The design brief landed as blueprint **§18
> "Switch routing — the model picks the branch"** (owner-chosen: design brief before implementation).
> It rules the surface form (`|> switch <col> { 'a' => <pipeline>, else => <pipeline> }`, `switch`/
> `else` as contextual idents so the keyword freeze does not move), per-row-batched-per-arm routing,
> discriminant-typed exhaustiveness failing at PREVIEW, the previewed-union commit envelope, the
> stage-admission argument, and `PipeOp` 19→20 (MINOR). The five owner calls at the end of §18 were
> ratified by the owner's direction to implement on this branch.

## Overview

Design and implement the mission's routing capability: a pipe stage where **the model's output
selects which downstream branch runs** — "switch predicate in pipe to let AI choose tool to
call". The disciplined shape, per the two-layer model and the one-seam thesis: a transform whose
OUTPUT is a closed enum-like column (a refined type), followed by a **switch construct that
routes rows to one of several declared sub-pipelines by that column's value**. The model call
stays inside the transform seam (no new model-call path); the switch itself is a pure, planable
stage-algebra construct over declared alternatives — AI chooses *among* pre-declared tools, it
never invents an effect. This is a language-design + implementation ticket: grammar, AST, plan
lowering, eval, and the governance locks (pipe-variant count, keyword freeze) move together.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/type-driven-design.md` — the routing column is a refined/enum type; exhaustiveness (all declared arms + default) is checked at PREVIEW
- `workaholic:design` / `policies/defense-in-depth.md` — each arm's effects remain individually previewed and gated; model choice never widens the effect set beyond what the statement declared
- `workaholic:design` / `policies/access-control.md` — arms touch only paths the policy layer grants; routing cannot escalate

## Key Files

- `packages/qfs/crates/parser/src/grammar.rs` - pipeline op grammar the switch construct extends
- `packages/qfs/crates/parser/src/ast.rs` - Pipeline/Op AST + the declared sub-pipeline arms
- `packages/qfs/crates/pushdown/src/lower.rs` - plan lowering for branch alternatives
- `packages/qfs/crates/core/src/eval.rs` - row routing at eval
- `docs/blueprint.md` - §15 (one seam), the two-layer chapter (stage admission test), pipe-variant/keyword locks

## Related History

Only the base transform-predicate design brief exists; the routing branch was never designed. Pipeline-valued lambdas (adopted-with-plan) is the natural representation for arms.

- [20260708002100-transform-predicate-design-brief.md](.workaholic/tickets/archive/work-20260707-180554/20260708002100-transform-predicate-design-brief.md) - the settled transform design this extends
- [20260709104259-pipeline-valued-lambdas-decision.md](.workaholic/tickets/archive/work-20260709-023822/20260709104259-pipeline-valued-lambdas-decision.md) - adopted genericity axis: abstraction over pipelines — the arm mechanism
- [20260709104255-two-layer-model-stage-admission-test.md](.workaholic/tickets/archive/work-20260709-023822/20260709104255-two-layer-model-stage-admission-test.md) - the admission test the new stage must pass

## Implementation Steps

1. **Design first** (blueprint section before code): surface form (e.g. `|> switch <col> { 'a' => <pipeline>, 'b' => <pipeline>, else => <pipeline> }` — exact syntax ruled against the keyword freeze and the pipeline-valued-lambdas plan), semantics (per-row vs whole-relation routing — rule it), exhaustiveness, and the stage-admission argument. Update the pipe-variant and keyword locks deliberately in the same change.
2. Parser: grammar + AST for the switch stage and its arm pipelines; parse tests including the freeze-test updates.
3. Plan: lowering to branch alternatives, forced-local like transform; PREVIEW shows every arm's effect plan (the union is the statement's declared effect set).
4. Eval/exec: route rows by the discriminant column; hermetic tests with a mock transform producing the discriminant, asserting each arm receives exactly its rows and non-taken arms with effects do not fire — while still having been previewed.
5. Docs: blueprint section, language reference (gen-docs), cookbook recipe "let the model pick the tool" (transform → switch → per-arm write), gen-skills; plugin version fields bump (taught-surface change → minor).

## Quality Gate

**Acceptance criteria**

- The switch stage parses, passes the stage admission test, and lowers with every arm previewed.
- Non-exhaustive switch (missing else over an open type) fails at PREVIEW.
- Hermetic end-to-end: mock model discriminant routes rows to the correct arm; untaken arms' effects never execute.
- Governance locks (pipe-variant count, keyword freeze) updated deliberately and green.

**Verification method**

- `cargo test --workspace` green including parser/lowering/eval switch tests and the updated locks; `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; the live routing round (real model choosing between two real arms, e.g. "save to Drive vs post to Slack") runs owner-attended and is recorded on this ticket.

## Considerations

- This is the mission's largest language change — if design review surfaces a fundamentally different shape (e.g. CASE-expression reuse instead of a new stage), record the alternatives in the blueprint before committing to grammar (`docs/blueprint.md`)
- Per-row routing with per-arm write effects interacts with the commit envelope — rule whether arms batch per-arm or interleave (`packages/qfs/crates/exec/src/lib.rs`)
