# 0004 — Package layout: `domain/` + `vendors/` + `entrypoints/`

**Status:** Accepted (2026-07-15)
**Ticket:** 20260715004234-repository-skeleton-and-dependency-contract.md
**Mission:** build-insightbrowser-on-the-plgg-family

## Decision

Each package lays out `src/` as:

```
src/
  domain/
    model/       types, brands, casters
    usecase/     pure logic
  vendors/       node:*, third-party adapters (the anti-corruption boundary)
  entrypoints/   CLI, SSR router, REST, MCP (thin program checkpoints)
  index.ts       re-exports domain only
```

Third-party and `node:*` imports may appear **only** under `vendors/` or
`entrypoints/`. plgg-family specifiers and the `qfs-viewer/*` self-alias are
domain vocabulary, importable anywhere. `scripts/gate-vendor-boundary.sh`
enforces this and self-tests its own red/green logic on every run.

## Reasoning

This layout **diverges from the stated wording** of
`workaholic:implementation` / `coding-standards`, which prescribes
`src/<Domain>/{model,service,dependency}`. The divergence is deliberate and
recorded here rather than taken quietly.

The evidence, gathered at ticket time:

- **No package in either reference repo uses the policy's stated layout.**
  plgg-md, plgg-server, and plggmatic use `<Domain>/{model,usecase}`; the
  newest gate-conformant packages (plgg-db-migration, plgg-bundle, the PoCs)
  use `domain/{model,usecase}` + `vendors/` + `entrypoints/`.
- **The machine-checked gate enforces the latter.** plgg's
  `.workaholic/constraints/architecture.md` names it "the reference layout",
  and `scripts/gate-vendor-boundary.sh` is written against it.
- A greenfield repository should not be born in a layout that its own
  enforcement does not recognise. Choosing the policy's wording would mean
  shipping a package the ported gate cannot check — which is worse for the
  policy's *intent* (isolate vendors) than diverging from its letter.

So: follow the gate, record the divergence. Where the policy's wording and its
machine-checked expression disagree, the checkable one wins, and the
disagreement gets written down instead of silently resolved.

The `vendors/`/`entrypoints/` split also carries the
`anti-corruption-structure` requirement directly: SSR, REST, and MCP are three
entry points over one domain, and the evidence the separation held is that each
can start the same domain procedure identically. Putting them under
`entrypoints/` makes that structural rather than aspirational.

## Alternatives considered

- **The policy's literal `<Domain>/{model,service,dependency}`.** Rejected: no
  sibling package uses it and the ported gate cannot check it. Literal
  compliance at the cost of enforceability is a bad trade.
- **`<Domain>/{model,usecase}`, matching plgg-md/plggmatic.** Rejected: it is
  the *older* family layout, predating the `vendors/`/`entrypoints/` split the
  gate checks. Familiar, but it would mean adopting a layout the family itself
  has moved on from — and this repo has no legacy to preserve.
- **A flat `src/`.** Rejected: nothing would distinguish a domain module from a
  vendor adapter, and the boundary gate would have nothing to enforce.

## Consequences

- The vendor-boundary gate is ported from plgg and passes unexempted from day
  one; `scripts/vendor-boundary-exemptions.txt` exists but is empty.
- Specs and `testkit/` are excluded from the production boundary: a test
  legitimately touches a real `node:fs` temp tree under the "test against the
  real thing" practice (`workaholic:implementation` / `test`). Domain purity is
  a claim about production structure, not about tests.
- `index.ts` re-exports `domain/` only. A consumer that could reach `vendors/`
  through the barrel would make the boundary decorative.
- When the scanner (ticket 2) adds the fs walk and the watcher, they land under
  `vendors/`, and the index/query logic stays pure under `domain/usecase/`.
