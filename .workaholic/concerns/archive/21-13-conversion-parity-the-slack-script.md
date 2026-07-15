---
type: Concern
origin_pr: 21
origin_pr_url: https://github.com/qmu/qfs/pull/21
origin_branch: work-20260705-032203
origin_commit: 1140091
created_at: 2026-07-05T13:59:54+09:00
last_seen: 2026-07-05T13:59:54+09:00
first_seen: 2026-07-05T13:59:54+09:00
concern_id: 13-conversion-parity-the-slack-script
severity: moderate
status: resolved
resolved_by_pr: 
resolved_by_commit: 
resolved_by_branch: work-20260705-173620
---

# §13 conversion parity: the Slack script twin diverges from the compiled driver on five named parks

## Description

Converting the compiled Slack driver to a script twin (`slack.qfs`, ticket 145138, commit b59a408) proved the self-hosting ratchet but surfaced five honest parity gaps: (1) envelope unwrapping — Slack wraps results in `{ok, messages}` so a tier-1 declared view decodes one envelope row, not the message rows the compiled driver unwraps; (2) nested cursor — `response_metadata.next_cursor` is nested but the tier-1 cursor descriptor only reads a top-level field; (3) weak typing — the declared json-decode yields json-object columns where the compiled driver yields typed columns; (4) dotted mount segments — Slack's dotted methods (`conversations.history`) make the mount path read as `/slack/conversations.history` rather than hierarchically; (5) POST body shape — `chat.postMessage` needs a specific `{channel, text}` body but the tier-1 MAP just passes the row through. A related surface gap: blueprint §13's own map-body example (`VALUES (ENCODE json)`) does not parse because `ENCODE` is a pipe op, not a value expression, so the declared-driver surface ticket (145136) substituted the driver's default codec instead of extending the effect grammar.

## How to Fix

Feed all six gaps back into blueprint §13 as named parks (already recorded on the tickets); implement a post-decode pipe op (e.g. an expand/field-extract) for envelope unwrapping and nested-cursor extraction, apply the `OF` type to the raw decode, support dotted/hierarchical mount addressing, add a per-map body-shape mapping, and either declare a map codec clause or add one to the grammar — only then does the runtime declared-vs-compiled byte-for-byte comparison become meaningful.

## Resolution (tier-2, branch work-20260705-173620)

All five parks are closed; the runtime declared-vs-compiled comparison is now meaningful and asserted:

1. **Envelope unwrapping** — the tier-2 view `… |> DECODE json |> EXPAND messages` unwraps `{ok, messages}` (blueprint tier-2 "a declared view IS its stored query", `qfs_exec::declared::eval_view_body`).
2. **Nested cursor** — the driver descriptor's `next 'response_metadata.next_cursor'` + the applier's dotted `cursor_from_body` walk (landed with transport honesty) follow Slack's nested cursor.
3. **Weak typing** — the `OF /type/slack/message` type shapes the delivered rows to the five compiled columns (`shape_to_type`). Rows are value-equivalent; only type/nullability metadata is late-bound (documented).
4. **Dotted mount segments** — the mount path (`/slack/history`, `/slack/post`) is decoupled from the dotted wire method (`conversations.history`, `chat.postMessage`) the body names.
5. **POST body shape** — the MAP `VALUES ({channel: row.channel, text: row.text})` is evaluated per incoming row into the exact `{channel, text}` wire body (`qfs_exec::declared::eval_map_body`, asserted on the recorded MockHttp request body).

The related surface gap (blueprint's old `VALUES (ENCODE json)` example not parsing) is closed by the `{…}` struct-literal map body, which parses as an expression constructor (no grammar change).

Proof: `slack_twin_read_is_row_equivalent_to_the_compiled_driver` (READ row-equivalence over a two-page nested-cursor envelope, `qfs_driver_slack::read_rows` vs the declared twin), `slack_twin_post_map_shapes_the_wire_body`, and `declared_map_write_evaluates_the_body_through_the_full_commit_stack` (crates/qfs/src/declared_driver.rs).

**Honest remaining write-side difference (NOT a park):** the compiled `chat.postMessage` additionally stamps a deterministic `client_msg_id` idempotency key the declarative MAP body does not express — a compiled-only refinement, documented in the test. It is orthogonal to the tier-2 acceptance (the declared twin produces the exact Slack-API body); no new concern is recorded.
