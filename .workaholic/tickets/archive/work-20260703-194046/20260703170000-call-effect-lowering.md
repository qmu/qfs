---
created_at: 2026-07-03T17:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 4h
commit_hash: 2defffa
category: Changed
depends_on: []
---

# A pipeline CALL never lowers to an effect: drive.copy (and likely mail.send / github.merge) silently reads

Live finding (owner-authorized parity check, 2026-07-03, drive-write-parity branch):
`/drive/my/<file> |> call drive.copy(parent_id => '…', name => '…') --commit` returned the FILE
CONTENT rows and copied nothing — no preview block, no `committed` key, no error. In
`qfs-core::eval`, `Statement::Query` always folds to `EvalValue::Relation`, and
`PipeOp::Call(_)` is a schema-preserving Shape node ("the call's effect, if any, is
materialised on the write side") — but no write-side path exists for a query pipeline, so
`build_plan` returns `Plan::pure()` and the CALL is dropped. `EffectVerb` has no Call variant.

Every documented pipeline CALL is suspect: `mail.send`, `github.merge`, `slack.post`,
`drive.copy` — the cookbooks present them as committed effects behind the irreversible gate,
and the resolve gate does vet them (`resolve_call`), but the terminal one-shot path apparently
never applies them. Investigate whether ANY entry path (shell, MCP, server) lowers a pipeline
CALL to an effect plan, then wire the one-shot path.

## Fix sketch

Lower a pipeline whose terminal op is `CALL` into an effect `Plan`: a Read node for the source
pipeline plus one `EffectKind::Call(proc)` node per matched row (or one node with args rows),
with the per-procedure irreversible flag from the driver's `ProcSig` (mail.send irreversible,
drive.copy not). The preview then shows the CALL effect honestly and the irreversible gate has
something to gate. Hermetic: golden plan-shape tests per procedure; the cookbook ratchet
already parses the recipes.

## Key files

- `packages/qfs/crates/core/src/eval.rs` (fold_query / eval_inner — the Query-with-CALL
  lowering), `crates/plan/` (Call nodes exist already), `crates/qfs/src/commit.rs` (the apply
  registry already routes `EffectKind::Call` to drivers — driver-gdrive decode_call is wired)
- Evidence: driver-gdrive `decode_call`/applier handle `drive.copy` correctly once a Call
  effect node reaches them (mock tests pass); only the lowering is missing.

## Quality Gate

- `<source> |> call <proc>(…)` previews a CALL effect node (irreversible flagged per ProcSig)
  and applies it under `--commit` (+`--commit-irreversible` when flagged).
- Live: `drive.copy` produces a real copy; the gated `mail.send` sends the owner's draft (the
  long-pending live-send verification, owner-attended).
- Cookbook CALL recipes re-verified; gen-docs/gen-skills regenerated.
