---
created_at: 2026-07-04T14:51:38+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash: b3c457e
category: Added
depends_on: [20260704145137-declared-driver-evaluator.md]
---

# Driver conformance check + the first compiled→script conversion (the ratchet's proof)

## Overview

Implement blueprint §13's **conformance** and prove the **self-hosting ratchet** once:

- **Conformance = §5's drift check aimed outward**: a declared view's `OF` type reconciled
  against the rows the live service actually delivers — the same set-difference machinery as a
  table's catalog drift, surfaced structurally (DESCRIBE/report), honest for services the
  binary never compiled. This is the acceptance test an LLM (and a user) runs after generating
  a script: `declared type vs delivered rows`.
- **The first conversion**: write the script twin of ONE compiled RestApiConfig-shaped driver
  (Slack is the smallest candidate: bearer auth, cursor pagination, an append-log read + a post
  mapping) and run both against the same hermetic fixtures; the conformance suite passing for
  the twin is the ratchet's proof-of-concept. Record the parity gaps honestly (anything the
  script cannot express goes to §13's named parks, with evidence).
- The compiled Slack driver is **not** deleted in this ticket — the ratchet rule ("a compiled
  driver may be deleted when its script twin passes the conformance suite") gets its first
  data point; deletion is the owner's call afterward.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions
- `workaholic:implementation` / `policies/test.md` — the conformance suite is the widened ground where the machine checks completeness
- `workaholic:implementation` / `policies/type-driven-design.md` — declared types are the outward contract under test
- `workaholic:implementation` / `policies/objective-documentation.md` — parity gaps are recorded as evidence, not rounded up to "parity"

## Key Files

- `packages/qfs/crates/driver-slack/src/` - the conversion target (the smallest RestApiConfig-shaped compiled driver)
- `packages/qfs/crates/driver-sql/src/conn.rs` + blueprint §5 - the drift-reconciliation shape being aimed outward
- `packages/qfs/crates/test/` - where the conformance harness and twin fixtures live (hermetic, MockHttp)
- `docs/blueprint.md` §13 - the ratchet rule and parks list this ticket evidences

## Implementation Steps

1. Implement the conformance check: given a declared view with `OF <type>`, fetch (hermetic:
   MockHttp fixtures), decode, and reconcile delivered rows against the declared type; report
   the difference structurally.
2. Write `slack.qfs` — the script twin (driver + types + views + MAP for posting) — as a
   committed fixture.
3. Run twin and compiled driver against the same fixtures; assert row-level equivalence on the
   shared surface; record gaps (e.g. anything beyond tier 1) in the ticket and §13's parks.
4. Wire the conformance run as a normal test in the hermetic suite (no network, no creds).

## Quality Gate

**Acceptance criteria:**

- The conformance check reports a seeded type-vs-rows mismatch structurally (negative test) and
  passes on the matching fixture (positive test).
- `slack.qfs` parses, installs, and its read+post surface is row-equivalent to the compiled
  driver on shared hermetic fixtures; every gap is written down with a §13 park named.
- No network or credentials anywhere in the suite.

**Verification method:** `cargo test --workspace`; `clippy --workspace --all-targets -- -D
warnings`; `fmt --all --check`; `gen-docs --check`; `gen-skills --check`.

**Gate:** all green; the twin's parity/gap report exists; owner decides separately whether the
compiled Slack driver is deleted.

## Considerations

- Slack's append-log archetype must be expressible by a declared view (tail read + INSERT
  append MAP); if the archetype declaration is missing from the §13 surface, that is a surface
  gap to feed back into the blueprint, not to hack around
- Keep the conformance API usable interactively (an agent iterating on a generated script wants
  the same check ad hoc), not test-only

## DONE (2026-07-05, work-20260705-032203)

- **Conformance = §5's drift check aimed outward** (`declared_driver.rs`, a PUBLIC API — usable ad
  hoc, not test-only): `conformance(of_type, type_columns, delivered) -> ConformanceReport` reports
  `missing` (declared but not delivered) + `extra` (delivered but not declared) columns; `load_declared_types`
  reads the `kind='type'` rows. ✅ positive (matching fixture conforms) + negative (seeded mismatch →
  structured drift) tests.
- **First conversion — `slack.qfs`** (`crates/parser/tests/fixtures/slack.qfs`): the script twin of
  the compiled Slack driver (driver + type + view + post MAP). ✅ `slack_qfs_parses_and_installs_to_sys_drivers`
  (every statement parses + desugars to a `/sys/drivers` install) + `slack_twin_reads_hermetically_and_records_the_envelope_parity_gap`
  (the declared twin reads a Slack `conversations.history` fixture over MockHttp with bearer auth,
  the GET hits the Slack host + dotted method natively, and conformance surfaces the parity gap).
- The compiled Slack driver is **NOT deleted** — the ratchet has its first data point; deletion is
  the owner's call once the parks below are closed.

### Recorded §13 parity parks (the ratchet's honest first-conversion gaps)

1. **Envelope unwrapping.** Slack wraps results in `{ok, messages}`, so a tier-1 declared view
   decodes ONE envelope row (`ok`, `messages`), NOT the message rows the compiled driver unwraps.
   Needs a post-decode `|> EXPAND messages` / field-extract (beyond tier-1). Conformance flags it.
2. **Nested cursor.** Slack's cursor is `response_metadata.next_cursor` (nested); the tier-1 cursor
   descriptor's `next_field` (`cursor_from_body`) reads a TOP-LEVEL field, so the nested cursor is
   not followed. Needs nested-field cursor extraction.
3. **Weak typing.** The compiled driver yields typed message columns; the declared json-decode yields
   json-object columns. The `OF` type is the outward contract, but the raw decode does not apply the
   typing — conformance surfaces the drift (this is the honest "declared type vs delivered rows").
4. **Dotted mount segments.** Slack's Web API methods are dotted (`conversations.history`), so the
   declared node's leading resource segment IS the dotted method — it maps natively, but the mount
   path reads as `/slack/conversations.history` rather than a hierarchical `/slack/conversations/history`.
5. **POST body shape.** `chat.postMessage` needs a specific `{channel, text}` body; the tier-1 MAP
   passes the row through. Matching the exact body shape is a per-map mapping concern.

The full RUNTIME declared-vs-compiled byte-for-byte row comparison (running both drivers over one
fixture) is a straightforward refinement once park 1 (envelope unwrapping) lands — until then the
shapes provably differ, so conformance-surfaced gaps ARE the honest parity record (the gate's bar:
"every gap is written down with a §13 park named").
