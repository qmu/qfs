# 0001 — The npm-only plgg-family dependency contract

**Status:** Accepted (2026-07-15)
**Ticket:** 20260715004234-repository-skeleton-and-dependency-contract.md
**Mission:** build-insightbrowser-on-the-plgg-family

## Decision

qfs-viewer is a standalone repository that consumes the plgg family from
the **npm registry** as published `^version` dependencies, and takes **no other
runtime dependency**. The family is never vendored, never patched in place, and
never consumed from a sibling checkout. Upstream gaps are filed upstream and
consumed here as a published `^version` bump.

The contract governs **runtime** (`dependencies`) only. `devDependencies` —
`typescript`, `plgg-bundle`, `plgg-test`, `@types/node` — are exempt: no
package could build or test without them, and conflating the two would reject a
buildable repository.

`scripts/gate-dependencies.sh` enforces this on every `check-all.sh` run.

## Reasoning

The decision that matters here is not "use plgg" — that follows from the
mission. It is **npm rather than a sibling checkout**, and the reasoning is the
part worth recording.

A monorepo, or a `file:` link to `../plgg`, would be materially easier
day-to-day: a change to plgg-md would be visible here instantly, with no
publish step. That convenience is exactly what we are refusing, for three
reasons:

1. **It is the only way to know the published artifact works.** plggmatic's
   repo split (`plggmatic/docs/0001-plggmatic-repo-split.md`) established this
   pattern, and its `check-all.sh` states the claim directly: a clean run
   *proves the published cross-repo contract resolves*. A `file:` link proves
   only that the source tree works — which is not what a user of
   `npx qfs-viewer` gets. The npx smoke check (`scripts/smoke-npx.sh`)
   exists for the same reason at the bin level.
2. **It forces upstream gaps to be fixed upstream.** When consuming from a
   sibling checkout, the path of least resistance for a missing seam is to
   reach into `../plgg` and add it locally — which silently forks the family.
   With a registry dependency, the same gap costs a real upstream PR and a
   release. That friction is the feature: it keeps the fix where every other
   consumer benefits (see ADR 0004, where exactly this trade-off is taken for
   plgg-md's heading seam).
3. **It keeps this repository honest about what it is.** qfs-viewer is the
   first *product* assembled on the plgg stack, not another package inside it.
   The registry boundary is what makes that claim testable rather than
   aspirational.

**The cost is real and accepted:** a cross-repo change is a round trip
(upstream PR → release → bump here), and under this environment's supply-chain
policy that round trip has a floor of seven days (ADR 0005). We take that cost
knowingly rather than dilute the contract.

## Alternatives considered

- **A monorepo package inside plgg/.** Rejected: qfs-viewer is a product
  built *on* the family, not a member of it, and folding it in would remove the
  one boundary that proves the published artifacts work. plggmatic already
  moved the other way, out of the monorepo, for the same reason.
- **`file:` links to sibling checkouts.** Rejected: fastest inner loop, but it
  proves nothing about what npm actually serves, and it makes silently forking
  the family the easy path. This is the option whose convenience we are
  deliberately declining.
- **Vendoring the parts we need.** Rejected outright: it converts every
  upstream improvement into a manual merge and guarantees drift.
- **Allowing narrowly-scoped third-party runtime deps** (e.g. a file watcher).
  Rejected for now: the family's own practice is "implement by default"
  (`workaholic:design` / `vendor-neutrality`), and the one place we expected to
  need it — the hot-reload watcher — is satisfiable with `node:fs` behind
  `vendors/`. If this is ever revisited, it should be a new ADR, not a quiet
  `package.json` edit; the gate makes that impossible to do quietly.

## Consequences

- `Result`/`Option` come from plgg rather than being hand-defined here.
  `workaholic:implementation` / `domain-layer-separation` says the Result type
  is "defined yourself", while `anti-corruption-structure`'s model layer
  explicitly permits "the project's base library", and plgg's own constraints
  doc names `Option`/`Result`/`Str`/`Dict` as sanctioned boundary-crossing
  types. Recorded here so it is not re-litigated per package.
- plgg-family packages are **domain vocabulary, not vendors** — importable
  anywhere, needing no anti-corruption translation. The vendor-boundary gate
  encodes exactly this (see `scripts/vendor-boundary-analyzer.mjs`).
- Only the deps a package actually **imports** are declared. Ticket 1's
  skeleton imports `plgg` alone, so `plgg`
  alone is declared; `plgg-view`, `plgg-md`, `plggpress`, and `plgg-cms` are
  added by the tickets that import them. Declaring unused dependencies would
  make the contract's own gate meaningless.
