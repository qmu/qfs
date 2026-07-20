# 0003 — Nothing is cached: a stale document is an incident

**Status:** Accepted (2026-07-15)
**Ticket:** 20260715004234-repository-skeleton-and-dependency-contract.md
**Mission:** build-insightbrowser-on-the-plgg-family

## Decision

qfs-viewer does not cache document responses. Every response carries
`cache-control: no-store, must-revalidate`. This holds for the local developer
server and for every hosted target (Cloudflare Worker + D1, Lambda + EFS +
sqlite), including any CDN in front of them.

The on-memory front-matter index is **not** a cache under this rule: it is the
serving model itself, rebuilt from the working tree and hot-reloaded on edit.
The distinction is authority — the index is derived from the current corpus and
invalidated by the watcher, whereas a cache serves a *past* answer on the bet
that it is still true.

## Reasoning

**This rule has no backing policy.** The engineering-policy corpus was searched
at ticket time: every `cache`/`stale` hit is an unrelated sense (AI familiarity
going stale, hand-drawn diagrams going stale). The mission asserts the rule, so
it binds only if written down here. That is why this ADR exists — without it,
the first performance-minded contributor would add a cache header and be
violating nothing.

The reasoning is a judgement about **what this corpus is for**:

qfs-viewer serves a repository's operational knowledge — tickets, missions,
concerns, specs, ADRs — to people and to AI agents making decisions right now.
The cost asymmetry is severe and one-directional:

- A cache **miss** costs milliseconds. The index is in memory; there is no
  database round trip to amortise. The thing a cache would save is small.
- A cache **hit on stale data** costs a wrong decision. An agent reading a
  superseded ADR over MCP, or a developer reading a closed concern as open, are
  incidents — and they are *silent* incidents, because a stale answer is
  indistinguishable from a correct one at the point of use.

Caching trades a large, silent correctness risk for a small, measurable latency
win. For a knowledge base that AI agents act on, that trade is backwards. Hence
the framing: **a stale document is an incident, not a performance win.**

There is a second-order reason. The mission's hosted architecture keeps
configuration and documents in R2, adaptive to qmu.app. A cache layer would
mean two sources of truth for "what does this document say" — and reconciling
them is exactly the class of bug that surfaces days later, in the one document
someone actually needed to be right.

## Alternatives considered

- **Cache with short TTLs (5–60s).** Rejected: it does not remove the failure,
  it makes it *intermittent* and therefore much harder to diagnose. "The doc
  was right when I checked" is the worst possible bug report.
- **ETag / conditional requests.** Rejected for now, though it is the only
  option with a real case: it preserves correctness (revalidation is
  mandatory) while saving bandwidth. It is declined because it buys little for
  an in-memory index and adds a validator-correctness surface — an ETag that
  fails to change on a content change reintroduces exactly the silent staleness
  this ADR forbids. Reconsider under measured hosted load, as a new ADR.
- **Cache immutable assets only** (hashed JS/CSS bundles). **Not rejected** —
  this ADR governs *documents*. Content-hashed static assets are outside it;
  when the client bundle lands, its caching is a separate decision.
- **Let the CDN decide.** Rejected: a default CDN policy is a cache we did not
  choose, in the one layer we cannot inspect from the server.

## Consequences

- Every `entrypoints/` handler sets `cache-control: no-store, must-revalidate`.
  Precedent to copy: `plgg-poc-portal` and `plgg-poc1-search`
  `entrypoints/serve.ts` already send exactly this.
- The SSR ticket asserts the header in its live probe, so the rule is checked
  rather than remembered.
- Hosted deployments must disable CDN caching explicitly; a default-on CDN
  silently violates this ADR, and the deployment ticket owns proving it does
  not.
- If latency ever becomes a real, measured problem, the answer is a faster
  index — not a cache.
