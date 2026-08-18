# The documentation map

Every documentation file in this repository, what it claims to cover, whether a machine writes it,
and what current state it does not reach.

**Verified against commit `52b0410` (`origin/main`), 2026-08-17, binary `qfs 0.0.108`.**

This page is a map, not a description of qfs. It exists so a reader can tell of any page whether it
is generated from the binary or hand-written, and what none of them covers. The two sides of it —
the documentation surface and the shipped system — were established independently: the pages by
reading them, the shipped system by reading the source and running the binary.

## How the surface was enumerated

```sh
git ls-files '*.md' | grep -v '\.workaholic/'
```

77 files at `52b0410`; this page is the 78th and carries its own row below, so every documentation
file in the repository appears here exactly once. The exclusion is deliberate and is the only one: the 750
markdown files under `.workaholic/` (root and `packages/qfs-viewer/`) are the engineering queue —
tickets, missions, stories, feedback, deployments — written by the workaholic workflow and read by
it, not documentation of qfs. They are addressed as a corpus by `qfs-viewer`, which is a different
relationship from being a page about the product.

## Who writes what

Three writers exist. Everything not named here is hand-written, with no generator and no drift
check.

| Writer | Command | Files it owns |
| --- | --- | --- |
| `qfs::docs` | `cargo run -p xtask -- gen-docs` | `docs/language.md`, `docs/drivers.md`, `docs/server.md` |
| `xtask::gen_skills` | `cargo run -p xtask -- gen-skills` | the 14 `plugins/qfs/skills/*/SKILL.md`, plus the `.claude/skills/<name>` symlinks |
| — | — | the other 61 files: hand-written |

**How far each is defended.** `gen-docs` drift is caught by the test suite: `qfs::docs`'s own
`docs_drift_golden` unit test calls `check_docs` and fails if a committed page differs from what the
binary renders, so `cargo test --workspace` (and therefore CI's `build + test` job) enforces it.
`gen-skills --check` and `check-migrations` have **no such test and no CI step** — CI (`ci.yml`)
runs `fmt`, `clippy`, `build`, `test`, two cross-compiles, the wasm32 host-core build, and
`qfs-viewer`'s `check-all.sh`, and never invokes `xtask`. Their drift is caught only when a person
or a routine runs the command. `check-migrations` additionally needs release tags to resolve a
baseline; with none it returns clean rather than failing (this clone carries 22 tags, `v0.0.71` …
`v0.0.108`, so it does resolve here).

## The published site (`docs/`, 36 files)

VitePress serves this tree (`srcDir` = `docs/`; `docker compose up docs` → `localhost:5173`). The
"In nav" column is against `docs/.vitepress/config.mts`: a page that is not there is reachable only
by URL or by search.

### Generated from the binary (3)

| Page | Claims | In nav | Does not reach |
| --- | --- | --- | --- |
| `docs/language.md` | The full grammar: reserved keywords, the two-layer model, codecs | yes | Nothing — it is the binary's own keyword set, rendered |
| `docs/drivers.md` | Per-mount archetype, capabilities, procedures, prelude aliases, pushdown; the builtin codec set | yes | The **compiled** describe registry only: 13 mounts (`/drive` `/fs` `/github` `/google-analytics` `/hosts/<host>/claude` `/local` `/mail` `/r2` `/rest` `/s3` `/sys` `/transform` `/type`). `/sql`, `/git` and `/cf` ship as driver crates but need a registered catalog/repo before describe resolves a node, so they are absent from the catalog; every **declared** driver (`/chatwork`, `/cloudflare`, `/slack`) is absent by construction, since a declaration is data an operator installs, not a compiled mount |
| `docs/server.md` | The `CREATE endpoint\|webhook\|trigger\|job\|view\|policy` binding forms, the policy model, the two deployment targets, materialized-view refresh | yes | The HTTP surface as routes; the MCP face; the embedded dashboard and the console |

### Hand-written guide and cookbook (33)

| Page | Claims | Last change | In nav | Does not reach |
| --- | --- | --- | --- | --- |
| `docs/index.md` | Site home (VitePress `layout: home`): the pitch and the three entry links | 2026-07-15 | home | — |
| `docs/README.md` | "Here's the map" — a hand-maintained index of the site, plus the objective-documentation `rg` scans for doc review | 2026-07-15 | **no** | Stale in three ways: it links a `qfs connection` subcommand that is now `connect` / `disconnect`; it points at "the ADR pages in the sidebar", and `docs/` carries no ADR pages at all (the ADRs live under `packages/qfs-viewer/docs/adr/`); and it lists neither `chatwork` nor `cloudflare` among the cookbook articles |
| `docs/blueprint.md` | The one living design document: intended design of the whole system, section by section, each headed with its own status (`implemented` / `blueprint` / `parked` / mixed) | 2026-08-16 | yes | Intent, by design. §14b/§14c are the only place `packages/qfs-viewer/` reaches the site. Its per-section status markers are the existing separation of fact from intent and are trusted here as the section author's claim, not re-verified page-wide |
| `docs/roadmap.md` | Where qfs is going next; states plainly that it is the plan, not what works today | 2026-07-18 | yes | — (correctly labelled) |
| `docs/query-cookbook.md` | A worked catalogue of queries, most in the *target* grammar; each recipe tagged `grammar=core\|extended` | 2026-07-25 | yes | Direction, not current fact — and it says so. Ratcheted by `crates/test/tests/roadmap_cookbook.rs`: ≥250 tagged recipes, of which ≥154 must parse on today's grammar |
| `docs/guide/installation.md` | Getting the binary; nothing to configure to start | 2026-07-15 | yes | — |
| `docs/guide/getting-started.md` | Zero to a local query, a conversion, a database query, with real output | 2026-07-25 | yes | — |
| `docs/guide/concepts.md` | Paths, the four archetypes, pipe-SQL, preview vs commit, cross-service joins | 2026-07-25 | yes | — |
| `docs/guide/design-snapshot.md` | The current operating model: mental model, the two state stores, defined paths and mounts, accounts and OAuth apps, `/sys`, DDL history, dump/restore, declared drivers and automation, the operational gates | 2026-07-15 | yes (twice) | The system as **built** — no crate, module, or internal seam appears. This is the operator's current state; the architecture page is its counterpart for a contributor |
| `docs/guide/module-system.md` | How qfs is extended without recompiling: registries, declared drivers | 2026-07-27 | yes | — |
| `docs/guide/account-model.md` | Untangling login / permission / service into the account model | 2026-07-15 | yes | — |
| `docs/guide/operator.md` | The operator identity `qfs init` records | 2026-07-15 | yes | — |
| `docs/guide/passphrase.md` | The QFS passphrase and unlocking the credential vault | 2026-08-13 | yes | — |
| `docs/guide/connect.md` | Connecting a service: what needs no setup, what needs an account | 2026-08-13 | yes | — |
| `docs/guide/connections.md` | What a connection is; storing credentials encrypted and scoping access | 2026-07-15 | yes | — |
| `docs/guide/cli.md` | CLI reference: the subcommands and flags | 2026-08-13 | yes | Written before `agent` and `view` shipped; the binary now carries 21 subcommands (`run` `describe` `skill` `dump` `restore` `plan` `apply` `serve` `connect` `disconnect` `init` `host` `app` `account` `vault` `auth` `identity` `invite` `job` `agent` `view`), and nothing checks this page against the clap tree the way `faq_cli_surface.rs` checks the FAQ |
| `docs/guide/shell.md` | The FTP-like interactive shell | 2026-07-15 | yes | — |
| `docs/guide/chatwork-benchmark.md` | The recorded declared-driver benchmark: My Drive, a Chatwork room, a previewed Slack request | 2026-07-15 | yes | A dated benchmark, not a reference |
| `docs/security/threat-model.md` | The threat model of one binary holding long-lived tokens | 2026-07-15 | yes (collapsed) | — |
| `docs/cookbook/index.md` | Cookbook overview and routing to the per-service articles | 2026-07-15 | yes | — |
| `docs/cookbook/gmail.md` | Reading and triaging Gmail over `/mail` | 2026-07-15 | yes | — |
| `docs/cookbook/gdrive.md` | Reading, writing, organizing Google Drive over `/drive` | 2026-07-17 | yes | — |
| `docs/cookbook/databases.md` | Querying and modifying SQL over `/sql/<conn>/<table>` | 2026-07-18 | yes | — |
| `docs/cookbook/git.md` | The versioned file tree and history over `/git` | 2026-07-18 | yes | — |
| `docs/cookbook/github.md` | PRs and issues over `/github`, merge behind the irreversible gate | 2026-07-25 | yes | — |
| `docs/cookbook/slack.md` | Channel messages, files, posting over `/slack` | 2026-08-16 | yes | — |
| `docs/cookbook/files.md` | Local files and S3/R2 objects, codec conversion | 2026-07-22 | yes | — |
| `docs/cookbook/cross-service.md` | One query spanning more than one service | 2026-08-16 | yes | — |
| `docs/cookbook/automation.md` | The server side: jobs, triggers, endpoints, cached views | 2026-08-16 | yes | — |
| `docs/cookbook/faq.md` | Operator "how do I…" and troubleshooting; exit codes | 2026-08-16 | yes | The one page with a CLI-surface guard: `crates/cmd/tests/faq_cli_surface.rs` walks the real clap tree and fails if a subcommand path or long flag it cites no longer exists |
| `docs/cookbook/chatwork.md` | The declared `/chatwork` driver, written in the query language itself | 2026-08-03 | **no** | Generates the `qfs-chatwork` skill, so an agent has it and a site reader does not |
| `docs/cookbook/cloudflare.md` | The declared `/cloudflare` driver: zones, DNS, KV, Queues, D1 | 2026-07-25 | **no** | Generates the `qfs-cloudflare` skill, same asymmetry |
| `docs/documentation-map.md` | This page: the documentation surface mapped against what ships today, dated and pinned to a commit | 2026-08-17 | yes | Nothing checks it; see *Maintaining this page* |

## Contributor documentation in `packages/qfs/` and at the root (5 files)

All hand-written. None is on the docs site.

| File | Claims | Last change | Does not reach |
| --- | --- | --- | --- |
| `README.md` | The qfs README: one grammar for every external service, install, the loop, the SemVer policy | 2026-07-15 | — |
| `CLAUDE.md` | Agent guidance: what the monorepo is, the build/test gates, the generators, the per-PR patch bump, the release path | 2026-08-17 | — (was: named `crates/qfs/tests/cookbook_skills.rs` for the recipe ratchet, a path that never existed. The ratchet itself is real and does both checks it claims — it lives at `packages/qfs/xtask/tests/cookbook_skills.rs`, moved there from `crates/test/tests/` on 2026-08-16 when the column half started needing the compiled describe registry. Corrected 2026-08-17, with the ratchet's coverage limits now stated in both places) |
| `packages/qfs/ARCHITECTURE.md` | The crate-boundary rules of the workspace: crate map, dependency spine, tokio confinement, decisions D1/D2, boundary rules, wasm-friendliness, cross-compile status, lints | 2026-07-15 | **The crate map is 20 crates against 48 on disk.** It predates `qfs-exec`, `qfs-http`, `qfs-mcp`, `qfs-oauth`, `qfs-store`, `qfs-session`, `qfs-identity`, `qfs-tunnel`, `qfs-watchtower`, `qfs-host`, `qfs-provision`, `qfs-skill`, `qfs-crypto-core`, `qfs-directory`-through-`qfs-driver-type` and the rest of the driver family. Its lints section names `clippy --all-features`, which `CLAUDE.md` now forbids (the `qfs-host` features are mutually exclusive). Boundary rules and decisions D1/D2 still hold |
| `packages/qfs/crates/test/README.md` | `qfs-test`, the dev-dependency-only offline harness (no creds, no sockets) | 2026-07-15 | One crate's own README |
| `packages/qfs/crates/skill/assets/SKILL.md` | The AI operating procedure embedded into the binary and printed by `qfs skill`; the source of the golden example corpus | 2026-08-13 | Hand-written but machine-proven: every worked example in it parses, evaluates, and matches a checked-in PREVIEW golden (`crates/skill/tests/golden_corpus.rs`) |

## Container and deployment READMEs (3 files)

Hand-written, none on the site, each scoped to one box.

| File | Claims |
| --- | --- |
| `containers/live-round/README.md` | The isolated box for the who-am-I live round of ticket `20260719101204` |
| `containers/claude-live-round/README.md` | The isolated box for the Claude-session live legs |
| `deploy/dev/README.md` | The `podman compose` Postgres + MariaDB dev stack with a seeded `widgets` table |

## `packages/qfs-viewer/` (20 files)

All hand-written. **None of these 20 files is reachable from the docs site**; the package appears
there only inside `docs/blueprint.md` §14b/§14c, as design.

| File | Claims | Does not reach |
| --- | --- | --- |
| `packages/qfs-viewer/README.md` | What qfs-viewer is: a markdown knowledge browser on the plgg family, SSR HTML + REST API + MCP; its two users; the dev scripts; a Working / Not built status table | The status table is stale in the direction that matters: it lists MCP, server-rendered documents, heading numbering, the column-accretion UI and tag groups as **not built**, while `src/entrypoints/` carries `mcp.ts`, `mcpTools.ts`, `document.ts`, `columns.ts`, `edit.ts` and `src/domain/` carries `Numbering.ts`, `tagGroups.ts`, `Trail.ts`. Its dependency list (`plggpress`, `plgg-cms`) does not match `package.json` (`plgg`, `plgg-mcp`, `plgg-md`, `plgg-server`, `plgg-view`, `plggmatic`) |
| `packages/qfs-viewer/CLAUDE.md` | Agent guidance for the package: the no-escape-hatch rule, the npm-only plgg contract, plggmatic as the UI engine, the deploy and verify commands | Repeats the same outdated dependency list |
| `packages/qfs-viewer/packages/qfs-viewer/README.md` | The publishable package itself: the scan, the index, what it serves | — |
| `packages/qfs-viewer/packages/plggmatic/README.md` | plggmatic: the column-oriented UI framework, typed light/dark scheme, components as pure functions | — |
| `packages/qfs-viewer/workloads/README.md` | What a workload is (execution environment / infrastructure configuration) | — |
| `packages/qfs-viewer/workloads/development/README.md` | The development workload container serving this repository's own corpus on port 4100 | — |
| `packages/qfs-viewer/docs/adr/index.md` | The ADR index | — |
| `…/adr/0001-npm-only-plgg-family-contract.md` | Why the plgg family comes from npm, not a sibling checkout | — |
| `…/adr/0002-plggmatic-is-a-reference-not-a-dependency.md` | plggmatic's status, twice amended into "the UI engine, consumed from npm" | — |
| `…/adr/0003-no-caching.md` | Why nothing is cached: a stale document is an incident | — |
| `…/adr/0004-package-layout-domain-vendors-entrypoints.md` | The `domain/` + `vendors/` + `entrypoints/` layout | — |
| `…/adr/0005-pinned-toolchain-under-min-release-age.md` | The pinned toolchain under `min-release-age`; explicitly time-boxed for retirement | Its own schedule has passed (2026-07-16); the ADR still reads as live |
| `…/adr/0006-observability-under-the-no-dependency-contract.md` | Logs yes, OpenTelemetry no | — |
| `…/adr/0007-resolve-subsumes-cols.md` | `/resolve/<trail>` as the canonical address | — |
| `…/adr/0008-corpus-from-the-qfs-collection-path.md` | The corpus is served from qfs's markdown collection path; the in-process indexer retires | The one recorded seam between the two packages, and it is invisible from the qfs side of the docs |
| `…/adr/0009-qfs-is-found-not-bundled.md` | qfs is found on PATH, not bundled or fetched | Same: a real coupling documented only inside the viewer |
| `…/adr/0010-following-the-plggmatic-reference.md` | Five divergences from the plggmatic reference, settled | — |
| `…/plggmatic-semantics/dsl-v1-core.md` | The frozen v1 core of the plggmatic flow DSL | — |
| `…/plggmatic-semantics/poc-findings.md` | Two findings from driving plggmatic in a bounded host | — |
| `…/plggmatic-semantics/screen-structure-mission.md` | The screen-structure model semantics (carries mission frontmatter) | — |

## Generated Agent Skills (14 files)

Each is `plugins/qfs/skills/<name>/SKILL.md`, rendered by `gen-skills` from the `docs/cookbook/`
article of the same subject: Claude Code frontmatter (`name` + `description` from the article's
`skill_name` / `skill_description`) followed by the article body verbatim. Never hand-edit one —
edit the article and regenerate. What each covers is its article's row above.

| Skill | Source article |
| --- | --- |
| `plugins/qfs/skills/qfs/SKILL.md` | (its own source: the base skill's article body) |
| `plugins/qfs/skills/qfs-cookbook/SKILL.md` | `docs/cookbook/index.md` |
| `plugins/qfs/skills/qfs-gmail/SKILL.md` | `docs/cookbook/gmail.md` |
| `plugins/qfs/skills/qfs-gdrive/SKILL.md` | `docs/cookbook/gdrive.md` |
| `plugins/qfs/skills/qfs-databases/SKILL.md` | `docs/cookbook/databases.md` |
| `plugins/qfs/skills/qfs-git/SKILL.md` | `docs/cookbook/git.md` |
| `plugins/qfs/skills/qfs-github/SKILL.md` | `docs/cookbook/github.md` |
| `plugins/qfs/skills/qfs-slack/SKILL.md` | `docs/cookbook/slack.md` |
| `plugins/qfs/skills/qfs-files/SKILL.md` | `docs/cookbook/files.md` |
| `plugins/qfs/skills/qfs-cross-service/SKILL.md` | `docs/cookbook/cross-service.md` |
| `plugins/qfs/skills/qfs-automation/SKILL.md` | `docs/cookbook/automation.md` |
| `plugins/qfs/skills/qfs-faq/SKILL.md` | `docs/cookbook/faq.md` |
| `plugins/qfs/skills/qfs-chatwork/SKILL.md` | `docs/cookbook/chatwork.md` |
| `plugins/qfs/skills/qfs-cloudflare/SKILL.md` | `docs/cookbook/cloudflare.md` |

## What ships today

Read from the source and the binary at `52b0410`, not from any page above.

| | |
| --- | --- |
| Version | `qfs 0.0.108` (`packages/qfs/crates/qfs/Cargo.toml`), tag `v0.0.108` present |
| Workspace | 48 crates under `packages/qfs/crates/`, plus `xtask` and one throwaway `spikes/parser-spike` |
| CLI | 21 subcommands (listed in the `docs/guide/cli.md` row above); no subcommand starts the interactive shell |
| Compiled driver crates | 16 (`driver-cf` `driver-claude` `driver-directory` `driver-fs` `driver-ga` `driver-gdrive` `driver-git` `driver-github` `driver-gmail` `driver-http` `driver-local` `driver-objstore` `driver-sql` `driver-sys` `driver-transform` `driver-type`), over the `qfs-driver` contract |
| Describe catalog | 13 mounts (see the `docs/drivers.md` row); `/sql` `/git` `/cf` are registration-gated, declared drivers are operator-installed data |
| Codecs | `csv` `json` `jsonl` `md` `toml` `yaml` |
| Faces | the CLI, the interactive shell, the HTTP listener (`qfs serve`), the MCP endpoint composed into it, the embedded SPA dashboard, and the loaded-not-embedded console |
| State | two SQLite stores at `$XDG_CONFIG_HOME/qfs/` (else `~/.config/qfs/`) — `system.db` and `project.db` — with 32 embedded migration bodies, plus the encrypted credential file `credentials` beside them |
| Gates | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` (plus the two mutually-exclusive `qfs-host` feature builds), `cargo build --workspace`, `cargo test --workspace` plus that suite's `qfs` lib half re-run serialised with `XDG_CONFIG_HOME` unset (the config-home isolation detector), two cross-compiles, a wasm32 host-core build, and `packages/qfs-viewer/scripts/check-all.sh` |
| Release | tag `v*` → `.github/workflows/release.yml` builds four native tarballs (`x86_64`/`aarch64` musl and Darwin) via `xtask dist`, each with a `.sha256`, and publishes a GitHub Release that `install.sh` consumes. The Workers wasm artifact is parked |
| qfs-viewer | one publishable package (`bin/qfs-viewer.mjs`, `qfs-viewer serve`) plus `plggmatic` as a sibling; entry points `cli` `serve` `api` `root` `document` `columns` `edit` `mcp`; it locates a `qfs` binary rather than bundling one |

## The gaps

Each is an area of the shipped system that no page covers, or a page that describes what the code no
longer does. These are the map's conclusions.

**Uncovered areas of the shipped system**

1. **The system as built.** No page describes the 48-crate workspace, how a statement travels
   through it, or which crate owns which stage. `ARCHITECTURE.md` is the only attempt and is 28
   crates behind; it is also outside the docs site. A contributor or agent asking "where does a
   query get planned, and which crate owns pushdown" must read the workspace.
2. **The repository itself.** No page describes the monorepo: that it holds two projects, which one
   is the product, what each gate command proves, which files a generator owns, or how a change
   becomes an installable release. That knowledge exists only in `CLAUDE.md` (agent guidance, and
   inaccurate on the ratchet) and in the workflow files.
3. **`packages/qfs-viewer/` at all.** Half the monorepo reaches the site only through
   `blueprint.md` §14b/§14c, i.e. as design. A reader of the documentation would not learn the
   package exists, what it runs as, or that it finds and shells out to `qfs`.
4. **Two cookbook articles that are shipped to agents but not to readers** — `chatwork` and
   `cloudflare` generate skills and are absent from the site navigation.
5. **The faces beyond the CLI.** The generated server guide covers binding forms and deployment
   targets; nothing covers the MCP endpoint, the embedded dashboard, or the console as things an
   operator meets.

**Pages that describe what the code no longer does**

6. `packages/qfs/ARCHITECTURE.md` — a 20-crate map of a 48-crate workspace; `clippy --all-features`
   in its lints section.
7. ~~`CLAUDE.md` — the cookbook recipe ratchet it names does not exist.~~ **Resolved 2026-08-17**
   (ticket 20260817105331). The survey read the absence correctly and the conclusion wrongly: the
   path `CLAUDE.md` gave (`crates/qfs/tests/cookbook_skills.rs`) never existed, but the ratchet does
   — at `packages/qfs/xtask/tests/cookbook_skills.rs`, both checks real, run by
   `cargo test --workspace` and so defended by CI's `build-test` job. The stale paths in `CLAUDE.md`
   and `faq_cli_surface.rs` are corrected and the ratchet's coverage limits are now written down.
8. `docs/README.md` — a `qfs connection` subcommand, ADR pages that are not in this tree, and a
   cookbook list missing two articles.
9. `docs/guide/cli.md` — written before `agent` and `view` shipped, and unguarded.
10. `packages/qfs-viewer/README.md` and `CLAUDE.md` — a Not-built list contradicted by the tree, and
    a dependency list contradicted by `package.json`.
11. `packages/qfs-viewer/docs/adr/0005` — self-declared time-boxed, past its own retirement date.

## Maintaining this page

It is a dated survey, not a generated artifact: nothing checks it. Re-take it by re-running the
enumeration at the top and re-reading the "What ships today" facts from the source, then replace the
commit and date in the header. The claim it makes about any single page is only as current as that
header.
