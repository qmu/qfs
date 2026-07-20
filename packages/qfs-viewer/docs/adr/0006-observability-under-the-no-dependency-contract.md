# 0006 — Observability under the no-dependency contract: logs yes, OTel no

**Status:** Accepted (2026-07-15)
**Ticket:** 20260715004234-repository-skeleton-and-dependency-contract.md
**Mission:** build-insightbrowser-on-the-plgg-family

## Decision

qfs-viewer implements the **logs** half of `workaholic:implementation` /
`observability` and, for now, **does not** implement the metrics/traces half.

- **Logs** — structured JSON, emitted from each component at creation (not
  bolted on later), with finite timeouts and bounded retries at every IO edge.
  This is satisfiable in-repo with zero dependencies.
- **Metrics / traces** — the policy asks for emission in vendor-neutral
  standard formats (OpenTelemetry / OpenMetrics). Every practical path to that
  is a third-party runtime dependency, which ADR 0001 forbids. Not implemented.

## Reasoning

This ADR exists because two of the project's own rules genuinely conflict, and
the conflict should be **recorded rather than silently resolved by whichever
rule the implementer happened to read second**.

- `observability` says: build in logs, metrics, and traces from the very start;
  align output to OpenTelemetry / OpenMetrics rather than a vendor-locked
  format.
- ADR 0001 (from the mission) says: the plgg family from npm, and no other
  runtime dependency. The plgg family ships no OTel exporter.

Writing an OTLP exporter by hand is not a serious answer: OTLP is a real
protocol with a real spec, and a hand-rolled half-implementation would be
vendor-neutral in name only while costing more to maintain than the signal is
currently worth. `workaholic:design` / `vendor-neutrality` says "implement by
default", but it says so about things whose cost is proportionate — a heading
numberer, a file walk. A telemetry pipeline is not that.

Meanwhile the **logs** half is both satisfiable and the part that pays off
first at this stage: the failure modes we actually expect — a malformed front
matter fence, a file vanishing mid-scan, a watcher event storm — are all
diagnosed from structured logs, not from a latency histogram. The policy's
named anti-pattern (`Indexed 47 files` via `console.log`) is the thing to
avoid, and avoiding it costs nothing.

So: take the half we can take properly, decline the half we cannot, and say so
out loud. A silent skip would look identical to an oversight, and the next
person would either re-derive this reasoning or "fix" it by adding the
dependency the contract forbids.

## Alternatives considered

- **Add an OTel SDK as a runtime dependency.** Rejected: it breaks ADR 0001,
  which is the mission's central constraint. If observability ever outweighs
  the no-dependency contract, that is a mission-level decision — a new ADR that
  supersedes 0001 in part, not a `package.json` edit.
- **Hand-roll an OTLP exporter.** Rejected: disproportionate to the current
  signal, and a partial implementation is worse than none — it would claim
  standard-format compliance while not delivering it.
- **Emit metrics in an ad-hoc JSON format and call it done.** Rejected: it is
  precisely the vendor-locked, non-standard output the policy names as the
  thing to avoid, with the added harm of looking like compliance.
- **Defer all observability until it is needed.** Rejected: the policy is right
  that retrofitting instrumentation is far more expensive than building it in,
  and the logs half costs nothing now.

## Consequences

- The scanner (ticket 2) emits structured JSON logs for scan start / complete /
  error with counts and durations, and holds finite timeouts plus bounded
  retries around fs reads.
- No metrics or traces are emitted. **This is a known, accepted gap** — if a
  reviewer or an audit asks "where is the OTel emission", this ADR is the
  answer, not an omission to be fixed by reflex.
- Revisit when there is a concrete consumer for the telemetry (a hosted
  deployment with an SLO, an actual dashboard someone reads). A metrics
  pipeline with no reader is cost without signal.
- If plgg ever grows a vendor-neutral telemetry seam, this ADR should be
  reconsidered immediately — that would remove the conflict entirely rather
  than trade one rule off against the other.
