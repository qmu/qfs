---
created_at: 2026-08-17T00:11:10+00:00
status: done
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

## Final Report

Development completed as planned.

The gate now answers per declared **node**. `RestApiConfig` carries a second, optional table —
`nodes: Vec<NodeMap>`, each entry a node's mount-relative path *template* plus the verbs that node
itself declares — and `RestDriver::caps_for` / `write_irreversible` consult it first whenever a
mount declares one. `resources` (the leading-segment aggregate) is untouched and still what the
wire layer resolves a resource with, so every compiled `/rest` config keeps exactly its previous
answer; the two tests that pin the aggregate on purpose stand, extended to state both grains
rather than re-pointed.

**Step 2's shape decision, recorded:** the ticket offered "a per-template `ResourceMap` list" or "a
separate declared capability table consulted before the segment list", and the second was taken.
Re-keying `resources` would have changed what `resource_for_segment` — the applier's own resolution
and the compiled configs' contract — answers, to fix something only the declared gate got wrong.

**The match is a union over every declared template that addresses the path, not most-specific-wins.**
The apply seam (`RestApplyDriver::apply_one`) selects the **first** declared map matching the path
*for the verb being written*, so a verb is performable exactly when some matching node declares it.
A precedence rule here would have refused writes the applier would then have performed, and this
layer must not invent an ordering the seams below it do not have. No shipped declaration has two
templates addressing one concrete path today, so the rule is currently unobservable — it is stated
because the alternative was the tempting one.

### Verification

- New: `a_declared_mount_gates_per_node_not_per_leading_segment` and
  `a_node_template_addresses_its_own_path_only` (driver-http, the layer's own unit tests);
  `every_declared_node_answers_exactly_its_own_verbs` and
  `a_declared_node_does_not_inherit_a_sibling_nodes_verb` (qfs), both built from the SHIPPED
  `.qfs` bytes — the first walks **every** node of all three shipped declarations (slack,
  chatwork, cloudflare) and asserts the mount answers a verb *iff* a declaration addressing that
  path says so.
- Sharpened: `an_unperformable_declared_write_refuses_at_plan_time` now pins
  `remove /slack/W1/users` → `unsupported_verb` naming `SELECT`, with
  `remove /slack/W1/files/F1` and `INSERT INTO /slack/W1/C1/messages` as the controls.
- Ratchet proof: with `.with_nodes(...)` stubbed back to an empty list, all three qfs tests fail —
  they measure the fix rather than restate it.
- Gates: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `gen-docs --check`, `gen-skills --check`, `check-migrations` — all exit 0. `cargo test
  --workspace` is 486 passed / 1 failed, the one failure being
  `provision::tests::offline_run_engine_does_not_mount_server`, which reproduces identically on an
  unmodified `origin/main` checkout in this container and is filed as
  `20260817010139-offline-run-engine-test-depends-on-a-siblings-home-guard.md`.

### Discovered Insights

- **Insight**: The declared-driver stack already had a path-template matcher and two consumers of
  it (`read_facets.rs` for views, `apply_facets.rs` for maps, both through
  `qfs_exec::declared::match_template`) — the capability gate was the one seam still keyed on a
  bare segment, which is why `describe`/read/write agreed with each other and disagreed with the
  gate.
  **Context**: When a subsystem answers the same question at several seams, the defect tends to
  live in whichever seam speaks a different coordinate space. `driver-http` cannot depend on
  `qfs_exec`, so the template match is re-implemented in `config.rs` (five lines) rather than
  shared — the duplication is a layering consequence, and the two implementations are pinned
  against each other by the shipped-bytes walk in `every_declared_node_answers_exactly_its_own_verbs`.

- **Insight**: `RestApiConfig` is `Serialize`/`Deserialize` and reaches wire-shape snapshots, so the
  new field is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. A compiled mount's
  serialized config is byte-identical to before.
  **Context**: The declared/compiled split runs through this DTO: every addition for the declared
  side has to be invisible to the compiled side, and the two `serde` attributes together are what
  buy that.
