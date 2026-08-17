---
created_at: 2026-08-17T00:11:10+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260817-004052
---

# A declared mount's plan-time verb gate is leading-segment coarse, so it allows a write no `CREATE MAP` declares

## Overview

A declared driver's plan-time capability answer is derived per **leading path segment**, not per
declared node. `DeclaredDriver::resources` (`crates/qfs/src/declared_driver.rs`) folds every view
and every map into one `ResourceMap` per segment-after-the-driver-name, unioning their verbs, and
`RestDriver::caps_for` (`crates/driver-http/src/lib.rs`) looks the concrete path's segment up in
that list. So a node inherits the union of every verb declared anywhere under its leading segment.

Measured 2026-08-17 against the shipped `slack_driver.qfs`, whose every node lives under
`/slack/{ws}/…` and which therefore assembles into exactly one resource:

```
resources() => [("{ws}", [Select, Insert, Remove])]
```

`CREATE MAP REMOVE` is declared for `/slack/{ws}/files/{file}` only, and `CREATE MAP INSERT` for
`/slack/{ws}/{channel}/messages` only — yet the gate answers `REMOVE`, `INSERT` and `SELECT` for
every `/slack/<ws>/…` path alike. `REMOVE /slack/<ws>/users` passes a capability check that should
refuse it.

This is the residue of ticket `20260816213014`, which closed the other half: a write to a path that
routes to **no mount at all** used to preview as an ordinary effect and now refuses at plan time.
That ticket's acceptance also asked the routed refusal to name "the verbs `/slack/<ws>/files` does
declare"; it names the leading segment's aggregate instead, which for a one-resource declaration is
every verb the driver declares anywhere.

`20260816213014` also had to repair a second consequence of the same keying to land at all: no
concrete workspace id matches the literal token `{ws}`, so the lookup missed entirely and every
declared write on a parameterised mount — mapped or not — was refused `unsupported_verb;
supported: []`. The fix there was a wildcard arm in `RestApiConfig::resource_for_segment` (a
`{param}`-token resource matches any concrete segment), which restores the aggregate semantics the
layer intends. It deliberately did **not** make the answer per-node.

## Scope

**In scope:** resolve a declared mount's capabilities (and `write_irreversible`) against the
declared node **path templates** — the same templates `describe` and the read/write seam already
match — so the answer is the verb set declared for *that node*.

**Out of scope:**

- Compiled drivers, which answer capabilities from their own `Capabilities` per path already.
- The unrouted-target refusal (`20260816213014`, shipped).
- Rewriting the wire-request half of `RestApiConfig` — the applier does not resolve declared writes
  through `ResourceMap`, so this is a describe/gate-side change.

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — the gate must answer from what the
  declaration says about the node addressed, not from a union that happens to contain it.
- `workaholic:implementation` / `policies/coding-standards.md`.
- `workaholic:operation` / `observability` — a capability answer that over-reports is an instrument
  reporting something other than the system's state.

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs` — `resources()` / `resource_segment()`: where the
  per-segment aggregation is built, and where a per-template list would be built instead.
- `packages/qfs/crates/driver-http/src/config.rs` — `ResourceMap`, `resource_for_segment` and its
  `{param}` wildcard arm; the natural home for a `resource_for_path` that matches templates.
- `packages/qfs/crates/driver-http/src/lib.rs` — `caps_for` and `write_irreversible`, the two
  consumers.
- `packages/qfs/crates/qfs/src/declared_driver.rs` (tests) —
  `an_unperformable_declared_write_refuses_at_plan_time` pins today's coarse answer and is the test
  to sharpen; `cloudflare_declared_driver_loads_confined_with_two_source_registry` and
  `rest_config_lifts_auth_pagination_and_resources` pin the aggregate semantics deliberately and
  must be re-read, not merely re-pointed.

## Implementation Steps

1. Reproduce: assert that `REMOVE /slack/<ws>/users` passes the capability gate today, against the
   shipped declaration through `declared_describe_mount`.
2. Decide the shape: a per-template `ResourceMap` list plus a template matcher, or a separate
   declared capability table consulted before the segment list. Weigh it against the two existing
   tests that assert aggregation on purpose.
3. Implement, keeping `resource_for_segment` working for the compiled/literal REST configs that use
   it.
4. Test: per shipped declaration, each declared node answers exactly its own verbs; an undeclared
   verb on a declared node refuses naming them; a mapped write still plans.

## Quality Gate

**Acceptance criteria**

- `REMOVE /slack/<ws>/users` refuses at plan time against the shipped declaration, naming `SELECT`.
- `REMOVE /slack/<ws>/files/<id>` and `INSERT INTO /slack/<ws>/<channel>/messages` still plan.
- The cloudflare and chatwork declarations keep answering for every node they declare.

**Verification method**

- The new hermetic tests, built from the shipped `.qfs` bytes rather than a hand-written fixture.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-17 while driving `20260816213014`. Its
  step 1 measurement is what surfaced this: the probe written to prove the refusal named the
  declared verbs found the mount naming none, and the cause was the keying rather than the refusal.
- The `{param}` wildcard shipped with `20260816213014` is a *restoration* of the intended coarse
  semantics, not a design endorsement of them — it is the smallest thing that made a parameterised
  declared mount answer at all. Read it as the thing this ticket replaces.
