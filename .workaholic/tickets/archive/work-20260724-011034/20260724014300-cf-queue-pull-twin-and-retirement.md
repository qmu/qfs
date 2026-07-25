---
created_at: 2026-07-24T01:43:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort: 2h
commit_hash:
category: Removed
depends_on:
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# /cf queue-pull twin and retirement

## Overview

Blueprint §13.3's honest-tiering table records exactly one exception whose reason is "not yet
done", not "cannot be done": the compiled **`/cf` queue-pull** is a read-over-POST whose declared
spelling now exists (G1, shipped v0.0.85), but the compiled implementation is still present at
HEAD. Close the exception with the same twin-and-retire arc, deliberately kept out of the G1 ship
ticket as a mechanical follow-up:

1. Declare the queue-pull twin (the `|> POST` read shape over the queue-pull endpoint) in the
   cloudflare declaration.
2. Prove it row-equivalent on the existing wire fixture (the G1 ship already drives a declared
   queue-pull twin through the real tier-2 evaluator in its hermetic test — reuse/extend it as
   the equivalence gate).
3. Delete the compiled queue-pull path from `driver-cf` (the queue-pull only — `/cf` Artifacts
   stays compiled per G8, it is a git-repo surface).
4. Update the §13.3 honest-tiering table: the exception's status flips from "not yet done" to
   closed, so the table keeps its promise that no silent exception rides the conversions.

Independent of the slack tickets (`depends_on` empty) — it can land first or last in the night.

## Policies

- Blueprint §13.1 G8 — the git-shaped `/cf` Artifacts surface is NOT touched; only the REST
  queue-pull converts.
- Blueprint §13 twin-and-retire ratchet — equivalence before deletion.
- CLAUDE.md plugin re-versioning — if any skill-taught surface names the compiled queue-pull,
  the ticket shares the mission's plugin MINOR bump (do not bump twice; coordinate with ticket
  20260724014200 in whichever PR ships).

## Quality Gate

1. The declared queue-pull twin reads row-equivalent to the compiled queue-pull on the wire
   fixture (hermetic).
2. The compiled queue-pull code path is deleted; `/cf` Artifacts and the rest of driver-cf are
   untouched; workspace builds clean.
3. `docs/drivers.md` regenerated if the described surface changed (gen-docs --check green).
4. Blueprint §13.3's tiering table row for the queue-pull records the closure with the commit.
5. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- Queue CONSUMPTION semantics (ack/visibility) beyond the existing compiled pull's behavior are
  out of scope — equivalence to what exists, nothing more.

## Final Report

The §13.3 honest-tiering table's ONE "not yet done" exception is closed. The full twin-and-retire
arc ran in this slice.

**1. The declared twin.** `crates/skill/assets/examples/cloudflare.qfs` now declares the pull:

    CREATE TYPE cloudflare/queue_message ( id text PRIMARY KEY, body text, attempts int );
    CREATE VIEW /cloudflare/accounts/{account}/queues/{queue}/messages/pull OF cloudflare/queue_message AS
      /http/cloudflare/accounts/{account}/queues/{queue}/messages/pull
      |> POST { batch_size: 100 } |> DECODE json |> EXPAND result |> EXPAND messages;

Two statements, +1 pipe stage over a plain view — exactly the declaration-cost §13.1 G1 priced. The
Cloudflare pull envelope is `{ result: { messages: [ … ] } }`: `EXPAND result` flattens the envelope
object one level, `EXPAND messages` explodes the batch into one row per message.

**2. Equivalence, then deletion (the ratchet, in that order).** A two-sided test drove BOTH sides
over one shared hermetic fixture — the compiled `HttpApiBackend::queue_pull` over `MockExchange`,
projected through `QueueMsg::to_queue_row`, and the declared view through the real tier-2 evaluator
over `MockHttpClient` — and asserted identical rows. It was GREEN before a single line of compiled
code was removed. Only then was the compiled pull deleted, and the surviving committed test
`declared_queue_pull_twin_is_row_equivalent_to_the_compiled_pull` re-pointed at the RECORDED oracle
(the exact rows the compiled pull produced for that fixture), per the /markdown-retirement
precedent the ticket names. It still asserts the wire shape too: one POST, to `…/messages/pull`,
carrying the evaluated `batch_size` body.

**3. What was deleted.** `CfBackend::queue_pull` (trait method), `HttpApiBackend::queue_pull`,
`MockCfBackend::queue_pull` + `with_queue_msg` + its `queue_msgs` field, `RecordedCall::QueuePull`,
the `QueueMsg` DTO and `to_queue_row`, `CfDriver::queue_tail`, and the `/cf` queue read facet arm in
`read_facets::cf_scan`. `/cf` Artifacts and every other driver-cf surface are untouched, per G8.

**4. Honest surfaces, asserted not asserted-in-prose.** The compiled `/cf` queue capability set
dropped from `{INSERT, SELECT}` to `{INSERT}` — a phantom SELECT advertising a pull the driver can
no longer answer is exactly the honest-surfaces violation the policy forbids, so
`queue_is_append_only_after_the_pull_retirement` pins the new set. `queue_tail_schema` was renamed
`queue_append_schema` (it is now a DESCRIBE contract, not a read shape). The read facet's Queue arm
is a structured refusal naming where the surface moved, never an empty batch.

**5. Docs + skills + blueprint.** `docs/cookbook/cloudflare.md` gained the pull path row, a
consume-a-queue recipe, and a corrected declared-vs-compiled tip; `docs/cookbook/automation.md`
stopped teaching `/cf/queue/<queue>` tails. `gen-skills` re-rendered `qfs-cloudflare`/`qfs-automation`
SKILL.md from them. Blueprint §13.3's tiering row for the queue pull now records the closure with
what was deleted, and states plainly that no "not yet done" exception remains in the table.

**Plugin + binary versions** (shared with the rest of the mission, not bumped twice): plugin
0.15.0 → **0.16.0** across all four fields (a taught-surface break — the cloudflare skill stops
teaching the compiled pull), binary 0.0.89 → **0.0.90**.

**Gate:** `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings`
green; `cargo fmt --all --check` green; `gen-docs --check` and `gen-skills --check` both report in
sync; `check-migrations` green.

**Out of scope, honored:** queue consumption semantics (ack/visibility) beyond the existing pull's
behavior were not added — the twin reproduces what existed, nothing more.
