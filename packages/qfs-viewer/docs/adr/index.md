# Architecture Decision Records

The reasoning behind the non-obvious decisions. Per
`workaholic:implementation` / `objective-documentation`, the reasoning section
matters more than the decision: the decision can be read from the code, the
reasoning cannot.

| ADR | Decision |
| --- | --- |
| [0001](0001-npm-only-plgg-family-contract.md) | The plgg family from npm, and no other runtime dependency — why the registry boundary rather than a sibling checkout. |
| [0002](0002-plggmatic-is-a-reference-not-a-dependency.md) | plggmatic began as a design reference; the 2026-07-17 amendment makes the ported engine this package's UI engine — landed the same day as the registry dependency `plggmatic@^0.2.0` and the engine strip. |
| [0003](0003-no-caching.md) | Nothing is cached: a stale document is an incident, not a performance win. |
| [0004](0004-package-layout-domain-vendors-entrypoints.md) | `domain/` + `vendors/` + `entrypoints/`, diverging from the coding-standards wording to match the machine-checked gate. |
| [0005](0005-pinned-toolchain-under-min-release-age.md) | Pinned toolchain under `min-release-age=7`, bridged with upstream's own relocation remedy — time-boxed, with a retirement schedule. |
| [0006](0006-observability-under-the-no-dependency-contract.md) | Structured logs yes, OpenTelemetry no — the recorded resolution of a real conflict between two project rules. |
| [0007](0007-resolve-subsumes-cols.md) | `/resolve/<trail>` is the canonical, prefix-closed address of the column view; `?cols=` is subsumed and redirected, never emitted. |
| [0008](0008-corpus-from-the-qfs-collection-path.md) | The corpus is served from qfs's `/markdown/<name>/documents\|links` collection path behind the `collection` switch; the in-process indexer retires (legacy scan arm deleted 2026-07-31). |
| [0009](0009-qfs-is-found-not-bundled.md) | qfs is FOUND on `PATH` (or named in config), never bundled and never fetched — it is the user's credential-holding substrate, not our build tool; a missing one is a supported, loudly-explained state. |
| [0010](0010-following-the-plggmatic-reference.md) | The five divergences from the plggmatic reference, each settled: `multiColumn` cannot draw a markdown document (upstream seam owed), and the landmark rule both trees used left real pages with no `main`. |
