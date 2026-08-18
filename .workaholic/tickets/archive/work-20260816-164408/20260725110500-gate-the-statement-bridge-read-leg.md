---
created_at: 2026-07-25T11:05:00+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
type: bugfix
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on:
mission:
claim: work-20260816-164408
---

# Gate the statement bridge read leg with the same policy

## Overview

Minted mid-run by the drive of `20260724013000-enforce-policy-on-the-serve-read-path` (it was found
while establishing which serve faces `dispatch_inner` covers). The endpoint face is now gated; a
**second serve face is not**, and it is the more permissive of the two.

`POST /api/run` with `{"mode":"read"}` — the statement bridge's read leg (`run_response` in
`crates/qfs/src/dashboard.rs`, routed to `McpEngine::read_rows`, implemented in
`crates/qfs/src/mcp.rs`) — parses a **caller-supplied statement** and runs it straight through
`qfs_exec::block_on_read` against the serve engine and read registry. There is no policy resolution
and no gate on that path at all. It also hardcodes `RequestContext::anonymous()`, so the principal
the predecessor mission threaded never reaches it.

This is strictly wider than the endpoint face it sits beside: an endpoint serves one pre-parsed,
pre-registered query, whereas this leg reads whatever statement the caller sends — every mounted
source, any path. The mission's `## Experience` demands that one policy govern every face reached
through the serve seam so per-face permission drift is structurally impossible; today a caller who
is refused by an endpoint's policy can read the same path through `/api/run`.

Mitigating context, not a fix: the bridge binds loopback-only by default and is documented as
unauthenticated for this milestone. That is a deployment posture, not an authorization decision, and
it does not survive the bridge being exposed.

## Implementation direction

- Resolve a policy for the bridge and evaluate the read through the SAME machinery the endpoint face
  uses: `qfs_exec::scan_targets` for the statement's scan leaves, then
  `qfs_server::evaluate_reads_with_context`. Do not fork a second read-gating path — the whole point
  of `decide_effect` being shared is that there is exactly one decision procedure.
- Decide what a bridge request's policy IS. Unlike an endpoint there is no `policy:` field to
  resolve, so this needs a product answer: a named bridge policy in `/server/policies`, a setting,
  or the operator-local super-admin posture (`crates/qfs/src/sys.rs`). Note the parked "what may I
  administer" decision is explicitly NOT to be closed as a side effect — if the answer requires it,
  record the blocker rather than deciding it here.
- Thread the request's resolved principal instead of the hardcoded `RequestContext::anonymous()`,
  using the same `PrincipalResolver` seam the HTTP handler already takes.
- A denied bridge read must be the structured refusal shape, not an empty envelope — the same
  honesty rule `20260724013200` pinned for the endpoint face.

## Policies

- workaholic:safety — fail-closed: a face that reads on the caller's behalf without consulting a
  policy is the widest possible default, and a loopback bind is not an authorization control.
- workaholic:design / アクセス制御 — one policy governs every face; a second ungated read face is
  exactly the per-face permission drift the mission exists to make impossible.
- workaholic:implementation / machine-checkable-domain-gaps — the gate must be a typed evaluation
  reusing the shared decision procedure, with a test that fails if the bridge ever reads ungated.

## Quality Gate

1. A `POST /api/run` with `{"mode":"read"}` over a path the resolved policy does not grant is
   refused with the structured secret-free denial at a denied status, and the read executor is
   never reached — proven hermetically (scan count 0), the same shape as the endpoint-face test in
   `crates/http/src/tests.rs`.
2. The same request under a granting policy returns the §14 envelope unchanged, so the bridge's
   existing behaviour for a permitted read is untouched.
3. The bridge evaluates the REQUEST's resolved principal, not a hardcoded anonymous context — one
   test per direction (a `FOR user:` grant bites for that user and contributes nothing otherwise).
4. Workspace gates green: `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
   `cargo run -p xtask -- gen-docs --check`.

## Considerations

- The MCP `preview`/`commit` tools already route through `gate_plan` + the `IrreversibleGuard`; only
  the read leg bypasses the policy layer. Do not disturb the commit path while fixing the read one.
- `read_rows` runs on a dedicated OS thread because `block_on_read` builds its own current-thread
  runtime. The gate is pure and must run BEFORE that thread is spawned, so a denial costs no thread
  and touches no driver.
- If the "which policy governs the bridge" question cannot be answered without the parked
  super-admin split, record it as blocked and name that decision — the mission's scope ruling
  forbids closing it as a side effect.

## Ruling (developer, 2026-07-26)

**The bridge is governed by its own named policy row, resolved and enforced the way an endpoint's
policy now is.** This answers the open question the Considerations section named as potentially
blocking: it does **not** route through the parked super-admin split, and it does **not** reuse the
existing `api` row.

What this settles:

- `POST /api/run` resolves a **dedicated, named** bridge policy — not the endpoint-name lookup, and
  not the shared `api` row that today gates MCP, the dashboard and reconcile together.
- **Fail closed.** If that policy is absent or its reference dangles, the bridge denies, matching
  the no-policy and dangling-ref cases already frozen for the endpoint face in `9623d6f`.
- The hardcoded `RequestContext::anonymous()` goes away; the bridge adjudicates under the request's
  resolved principal, so an anonymous caller sees only `Anyone` + `Always` grants exactly as on the
  endpoint face.
- The loopback-only default bind stays what it is — a deployment posture, not the authorization
  control — and is not counted as mitigation once this lands.

Rejected alternatives, recorded so they are not re-litigated: treating a loopback caller as an
operator-local super-admin (it presumes the parked "what may I administer" ruling, which this
mission's scope forbids closing as a side effect), and widening the existing `api` row (it is the
coarse-grant defect that the compound concern `one-coarse-api-policy-row-for` now tracks at
urgent — reusing it would deepen exactly the risk being tracked).

The related concern `one-coarse-api-policy-row-for` (urgent) is the tracked risk this ticket
discharges on the read leg; splitting the remaining non-endpoint faces off the shared `api` row is
that concern's own remit, not this ticket's.

## Queue provenance — the `mission:` stamp was cleared on 2026-08-12

This ticket was minted under the mission **`what-a-principal-can-see-and-do-is-granted-by-policy`**, which closed `achieved` while the ticket
itself stayed unfinished. `plan-units.sh` excludes any mission-stamped ticket from the developer's
backlog **without checking whether that mission is still active** (`plan-units.sh:432` — a non-empty
mission relation is excluded as `mission_member`), and only *active* missions are offered as mission
units. A ticket stamped with a closed mission is therefore reachable by neither path, and this one
had been invisible to every `/drive` survey since the close.

The stamp is cleared so the ticket returns to the ordinary backlog — the same correction
`20260804173000` received when its own mission closed. The provenance lives here in prose instead.

**Still-open evidence (verified 2026-08-12, read-only):** The read leg is still ungated: `McpEngine::read_rows` parses a caller-supplied statement and runs it through `block_on_read` under a hardcoded `RequestContext::anonymous()`, with no policy resolution (`crates/qfs/src/mcp.rs`, the `mode: "read"` leg).

## Final Report

Development completed as planned. The bridge's read leg (`POST /api/run`, `mode: "read"`) now
resolves a **dedicated named policy row** and adjudicates the statement's scan leaves through the
*same* decision procedure the endpoint face uses, under the request's resolved principal — the
developer's ruling of 2026-07-26 implemented without forking a second read-gating path.

What landed:

- `ServeMcpEngine::read_policy()` resolves the `bridge` row out of the live `/server/policies`
  table. Fail-closed in **every** degraded direction: no live `/server` seam, an unreadable state
  lock, or an absent row all resolve to the default-deny policy. The `api` row is untouched — the
  ruling rejected widening it, and the `one-coarse-api-policy-row-for` concern is why.
- The gate is `qfs_exec::scan_targets` → `qfs_http::assert_select_allowed`, i.e. the endpoint
  face's own `evaluate_reads_with_context` seam. It is pure and runs **before** the read worker
  thread is spawned, so a denial costs no thread and touches no driver (pinned by a counting
  driver: `scan_count() == 0` on every refusal).
- `McpEngine::read_rows` gained a `ctx: &RequestContext` parameter; the hardcoded
  `RequestContext::anonymous()` is gone from both the gate and the executor call. `serve_dashboard`
  takes the context and `qfs serve` supplies it from the **same** injected `PrincipalResolver` the
  endpoint face reads (no resolver wired ⇒ anonymous, the same fail-closed default).
- A denied read is a structured, secret-free `policy_denied` at **403** (`engine_status` gained the
  arm), never an empty envelope at 200. The message names the governing row so an operator can act
  on it.

Quality Gate: all four items verified. (1) A denied `mode: "read"` refuses with the structured
denial and `scan_count() == 0` — `bridge_read_is_gated_and_denial_precedes_the_driver_scan`, plus
`a_bridge_with_no_declared_policy_row_reads_nothing` for the absent-row case the ruling named.
(2) The granted direction returns the §14 `{schema, rows, …}` envelope unchanged, and the two
reconcile end-to-end tests still converge through the bridge with a declared row. (3)
`bridge_read_gate_evaluates_the_resolved_principal_both_directions` — one `FOR user:alice` rule,
three directions (alice allowed, anonymous and bob denied). (4) Workspace gates green:
`cargo test --workspace` 2719 passed / 0 failed, `cargo clippy --workspace --all-targets -D
warnings` clean, `cargo fmt --all --check` clean, `gen-docs --check` / `gen-skills --check` /
`check-migrations` all in sync.

The parked super-admin split was **not** touched, as the ruling required.

### Discovered Insights

- **Insight**: The reuse the ticket demanded was available as a public function, not just as a
  pattern — `qfs_http::assert_select_allowed` already takes `(&[ScanTarget], &Policy,
  &DecisionContext)` and is exported for exactly this kind of caller. Only `decision_for` (the
  `RequestContext` → `DecisionContext` map) was missing from the export list.
  **Context**: The binary is pinned OFF a direct `qfs-server` dependency by the thin-entrypoint
  guard, so a second gate written "the obvious way" in `crates/qfs` would have had to re-export the
  enforcer through `qfs-mcp` and re-derive the ReadTarget mapping — a fork by construction. The
  serve-side door (`qfs-http` re-exporting what the composition root needs) is the pattern to reach
  for whenever the binary needs a policy decision.

- **Insight**: Gating this leg is a **breaking change for `qfs plan` / `qfs apply`**, and the
  failing tests said so before any reasoning did. Reconcile is the bridge's third client
  (`fetch_server_state` reads six `/server/<collection>` paths through `mode: "read"`), so a
  deployment that declares no `bridge` row can no longer be reconciled at all.
  **Context**: This is the ruling's intended posture, not a regression — "fail closed" over a face
  that reads whatever statement the caller sends. But it means the upgrade is not transparent: the
  cookbook's reconcile recipe now teaches the row, and an existing `config.qfs` must add it. Any
  future gate over a face an internal CLI drives will have the same shape — the test that breaks is
  the one telling you a client exists.

- **Insight**: The bridge's `commit_policy()` and `read_policy()` deliberately read **different**
  rows (`api` vs `bridge`), which looks like an inconsistency and is the opposite.
  **Context**: `api` is the coarse row the cookbook already teaches and that MCP, the dashboard and
  reconcile share; the tracked `one-coarse-api-policy-row-for` concern is precisely that coarseness.
  The ruling rejected reusing it here so the read leg could be split off cleanly, which is also the
  template for splitting the remaining non-endpoint faces off `api` later.
