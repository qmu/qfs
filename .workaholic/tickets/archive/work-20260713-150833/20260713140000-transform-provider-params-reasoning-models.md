---
created_at: 2026-07-13T14:00:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Transform provider layer breaks on current reasoning models (max_tokens param + thinking budget)

## Problem (found live, round 7, v0.0.59)

Proving text generation on OpenAI and Google surfaced two provider-layer issues, both rooted in
`crates/qfs/src/transform_providers.rs` assuming non-reasoning model behavior:

1. **OpenAI `max_tokens` rejected by reasoning models.** The body always sends `"max_tokens"`
   (line ~236). A reasoning model (`gpt-5-mini`, o-series) rejects it with **HTTP 400** — those
   models require `max_completion_tokens`. `provider 'openai' returned HTTP 400 (no usable
   completion)` is all the operator sees; nothing points at the parameter. `gpt-4o-mini` (a
   non-reasoning model) works with the current shape and was the round-7 OpenAI proof.

2. **Reasoning models spend the whole token budget on thinking → empty output → schema failure.**
   `effort 'low'` maps to `max_tokens: 256`. Google's currently-servable model for a new-user key
   is `gemini-flash-latest` (resolves to `gemini-3.5-flash`, a **reasoning** model — the 2.x flash
   names all return HTTP 404 "no longer available to new users"). At 256 tokens it burns ~all of
   them on `thoughtsTokenCount` and returns no content, which surfaces as the misleading
   `provider 'google' did not return JSON matching the declared OUTPUT schema`. `effort 'high'`
   (4096) leaves budget after thinking and returned a clean completion — the round-7 Google proof.

Both defaults were fine for the models available when the seam was written; current provider
model lineups have shifted under them.

## Fix

- OpenAI: send `max_completion_tokens` (not `max_tokens`) — it is accepted by both reasoning and
  non-reasoning chat-completions models — or detect a 400 naming the param and retry. Confirm the
  Anthropic/Google param names against current APIs too.
- Reasoning-model budget: either raise the `low`/default `max_tokens` mapping to leave post-thinking
  output room, or (better) map `effort` onto the provider's own reasoning-effort control
  (OpenAI `reasoning_effort`, Gemini `thinkingConfig`) so `low` means less thinking, not fewer
  total tokens.
- Error surfacing: an empty completion from a thinking model should say so ("model returned only
  reasoning tokens; raise effort/max_tokens"), not "did not return JSON matching schema".

## Key files

- `packages/qfs/crates/qfs/src/transform_providers.rs` — `openai_call`, `google_call`,
  `max_tokens_for`, the schema-mismatch error path
- provider param hermetic tests (assert the emitted body key per provider)

## Acceptance

- A reasoning OpenAI model (e.g. gpt-5-mini) and a reasoning Gemini model complete at `effort low`
  without a 400 and without an empty-output schema failure.
- The empty-thinking-output case produces an actionable error.

## Resolution (2026-07-13, branch work-20260713-150833)

Three fixes in `transform_providers.rs`:

1. **OpenAI param**: `openai_call` now sends `max_completion_tokens` (not `max_tokens`). The
   chat-completions API accepts it on BOTH reasoning (o-series, gpt-5) and non-reasoning models,
   while a reasoning model 400s on `max_tokens`. (Anthropic's Messages API keeps `max_tokens` — the
   correct param there; Gemini keeps `maxOutputTokens`. Confirmed both against current APIs.)

2. **Reasoning budget**: raised the `effort` ceilings so output room survives thinking —
   `low` 256 → 1024, default/medium 1024 → 2048, `high` 4096 (unchanged). Chose the ticket's
   "raise the ceiling" option over "map effort onto `reasoning_effort`/`thinkingConfig`": those
   controls are only valid on reasoning models, so sending them unconditionally would 400 a
   non-reasoning model (the same class of bug), and model-name sniffing to decide is fragile. A
   ceiling is not a target — a non-reasoning model still stops after what it needs, so raising it
   costs those models nothing while giving reasoning models post-thinking room.

3. **Error surfacing**: a new `require_completion` guard (called in all three providers before the
   JSON parse) turns an empty completion into an actionable
   "returned an empty completion — a reasoning model can spend the whole token budget on thinking …
   raise `effort`" error, replacing the misleading "did not return JSON matching schema".

New hermetic locks: `openai_sends_max_completion_tokens_not_max_tokens` (asserts the body key and
the ABSENCE of `max_tokens`), `an_empty_completion_is_an_actionable_error_not_a_schema_mismatch`,
and the updated `effort_maps_to_a_bounded_token_ceiling` (`low` = 1024).
