---
created_at: 2026-07-11T12:15:29+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash: e567332
category: Added
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Wire live model providers (Anthropic, OpenAI, Google) behind the transform seam

## Overview

The transform epic (T1–T4, v0.0.42) shipped the whole pipeline — DDL, plan spine, execution
routing, docs — but **no live provider is wired**: only the fail-closed `UnconfiguredProvider`
registers, and T4's live run was explicitly deferred. This ticket implements real `ModelProvider`
impls for the three major providers (Anthropic Messages API, OpenAI Responses/Chat API, Google
Gemini API) as binary-leaf concerns: constructed in the qfs binary only, secret resolved lazily
from the def's `secret_ref` (`env:`/`vault:`) at the call, requests riding the one `call_model`
funnel with its crate-private `CallProof` witness. Provider selection comes from the transform
def's non-secret `provider`/`model` columns. Simple text generation against each provider is the
mission's acceptance; the live rounds spend real tokens and are owner-attended.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/vendor-neutrality.md` — three vendors behind ONE domain seam (ModelProvider); vendor request/response types never leak past the provider impl
- `workaholic:safety` / `policies/standard.md` — API keys only as env:/vault: references resolved at call time; never in defs, DESCRIBE output, logs, or the reconcile SoT
- `workaholic:implementation` / `policies/observability.md` — finite timeouts and bounded retries on every provider call; structured, secret-free spans
- `workaholic:implementation` / `policies/functional-programming.md` — providers injected at assembly; the pure planning layers stay model-free

## Key Files

- `packages/qfs/crates/driver-transform/src/provider.rs` - ModelProvider seam, call_model funnel, CallProof witness, ModelRequest/ModelError, UnconfiguredProvider
- `packages/qfs/crates/qfs/src/transform.rs` - binary-side executor/backend; where real providers get constructed and secret_ref resolved
- `packages/qfs/crates/driver-http/` - reqwest confinement leaf (rustls, no default features) the provider HTTP should reuse rather than adding a second HTTP stack
- `packages/qfs/crates/types/src/transform.rs` - TransformMode / derive_mode (the three cardinality modes provider output must honor)
- `docs/blueprint.md` - §15 one-seam thesis and §11 dependency posture (no vendor SDKs — hand-rolled REST)

## Related History

The seam was designed for exactly this; the live half is the recorded deferral.

- [20260708192732-transform-execution-routing.md](.workaholic/tickets/archive/work-20260709-023822/20260708192732-transform-execution-routing.md) - T3: ModelProvider seam + executor at the commit boundary
- [20260708192733-transform-docs-versioning-live-run.md](.workaholic/tickets/archive/work-20260709-023822/20260708192733-transform-docs-versioning-live-run.md) - T4: live run DEFERRED, owner-gated — this ticket is that deferred half
- [20260709104300-transform-one-seam-lock.md](.workaholic/tickets/archive/work-20260709-023822/20260709104300-transform-one-seam-lock.md) - the enforced invariant every provider impl must satisfy

## Implementation Steps

1. Implement three provider structs in the binary leaf (no vendor SDKs — hand-rolled REST over the existing driver-http transport): Anthropic `/v1/messages`, OpenAI `/v1/responses` (or chat completions — pick and record), Gemini `generateContent`. Map ModelRequest {model, effort, mode, output schema, input rows} to each wire shape; parse output back to the declared OUTPUT schema rows.
2. Secret resolution: `env:NAME` from process env, `vault:name` via the existing vault read, resolved inside the call only; a missing/invalid ref fails closed as ModelError variants with no secret material in the message.
3. Registry: provider column value (`anthropic`/`openai`/`google`) selects the impl; unknown providers keep failing closed as Unconfigured.
4. Hermetic tests per provider with a mock transport: request-shape golden tests (auth header form, model field, schema-constrained output request) and response-parse tests including refusal/error mapping; assert Debug/log output of every type is secret-free.
5. Timeouts + bounded retry (respect Retry-After on 429) per the observability policy; per-call token/usage counts surfaced as non-secret span fields.
6. Docs: transform article gains the provider matrix (three providers, model naming, secret_ref forms); regenerate docs/skills; bump plugin version fields if the taught surface changes.

## Quality Gate

**Acceptance criteria**

- A `CREATE TRANSFORM … PROVIDER 'anthropic'|'openai'|'google'` def executes through its matching impl (hermetic mock transport), producing OUTPUT-schema rows.
- Wrong/missing secret_ref fails closed pre-network with a structured, secret-free error.
- The one-seam lock test still passes; no new model-call path exists.

**Verification method**

- `cargo test --workspace` green (new provider golden/parse tests, one-seam lock, secret-free Debug assertions); `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate. The live text-generation round — one real call per provider with the owner's real keys ("hello, one sentence" class prompt, bounded cost) — runs owner-attended per the 2026-07-11 policy answer and is recorded on this ticket; it is the mission's acceptance evidence.

## Considerations

- Provider APIs drift; hand-rolled REST needs a pinned API version header per provider and a golden-test per wire shape (`packages/qfs/crates/qfs/src/transform.rs`)
- Structured-output enforcement differs per provider (Anthropic tool-use vs OpenAI response_format vs Gemini responseSchema) — normalize behind the OUTPUT schema fold, record per-provider fidelity gaps
- Cost control for live rounds: cap max_tokens in the def-level defaults so an owner-attended tryout cannot run away

## Live Round Evidence

### Round 7 — text generation on every major provider (2026-07-13, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a). Keys from the owner's `.env` (`OPENAI_AI_KEY`,
  `GEMINI_API_KEY`; `ANTHROPIC_API_KEY` already proven).
- **Anthropic:** already live-proven in the T8 switch round (round 2) via the `triage` transform
  (claude-haiku-4-5). ✓
- **OpenAI (`gpt-4o-mini`, `secret 'env:OPENAI_AI_KEY'`):** a subject→reply transform returned a
  clean natural-language completion referencing the subject ("...regarding the file
  'qfs-round10.txt'."), `affected 1`. ✓
- **Google (`gemini-flash-latest` → gemini-3.5-flash, `effort 'high'`, `secret
  'env:GEMINI_API_KEY'`):** returned a clean natural-language completion referencing the subject,
  `affected 1`. ✓
- **Two provider-layer defects found and ticketed (20260713140000):** (1) qfs sends OpenAI
  `max_tokens`, which reasoning models (gpt-5-mini) reject with HTTP 400; gpt-4o-mini was the
  working proof. (2) The new-user Gemini key can only reach reasoning models (the 2.x flash names
  404 "no longer available to new users"), and at `effort 'low'` (256 tokens) the model spends the
  whole budget on thinking → empty output → the misleading "did not return JSON matching schema";
  `effort 'high'` (4096) fixed it. Direct provider curls confirmed the diagnoses (models.list
  showed the key valid; gpt-4o-mini + gemini-flash-latest both HTTP 200 directly).
- **Ticks acceptance:** "Simple text generation verified against every major provider (Anthropic,
  OpenAI, Google)".
- **Residue:** several probe transform definitions (`genoai`, `genglh`, and dead-ends
  `genopenai`, `gengoogle`, `gengoogle2`, `gengl`, `gengll`); `remove transform <name>` drops each.
