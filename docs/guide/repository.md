# The repository as it stands

What a contributor arriving at this repository has to know before touching anything: the two
projects it holds, how each is verified, which files a machine owns, and how a change becomes an
installable release.

**Read from the repository at commit `52b0410` (`origin/main`), 2026-08-17, binary `qfs 0.0.108`.**

This page is the reader-facing account. `CLAUDE.md` (repository root) and
`packages/qfs-viewer/CLAUDE.md` are the agent-facing ones, and they are deliberately shorter and
more imperative. The overlap is real and stated rather than silently doubled: where this page and a
`CLAUDE.md` describe the same gate, the command is the same command, and this page adds what the
command proves and what it is the only thing to catch.

## Two projects, one monorepo

Each project lives under `packages/<project>/`. Today there are two.

| | `packages/qfs/` | `packages/qfs-viewer/` |
| --- | --- | --- |
| What it is | `qfs`, one Rust binary — CLI, shell, HTTP server, MCP — exposing every external service through one filesystem-shaped pipe-SQL language | `qfs-viewer`, a TypeScript markdown knowledge browser: SSR HTML, a REST API, and an MCP server over a repository's markdown |
| Status | **The product.** Versioned, tagged, released | Experimental study work, publicly available, `0.0.1`, published to npm rather than released as a binary |
| Toolchain | Cargo workspace, `rust-toolchain.toml` pinned | npm, TypeScript, its own script runner |
| Gate | `cargo` commands from `packages/qfs` | one script: `./scripts/check-all.sh` |

Two things at the repository root belong to the qfs product rather than to the monorepo: `README.md`
is the qfs README, and `docs/` is the documentation site you are reading
(VitePress; `docker compose up docs` serves it at `localhost:5173`, the only service in
`docker-compose.yml`). The root
`package.json` exists solely for that site (`docs:dev`, `docs:build`, `docs:preview`, and the
three `docs:deploy:*` commands below).

The site is also **published, twice, without anyone running a deploy command**: a merge to `main`
puts it on `staging-qfs.qmu.co.jp`, and a `v*` tag puts the tagged commit's documentation on
`qfs.qmu.co.jp`. `docs/wrangler.toml` declares both environments (Cloudflare Workers static
assets) and `.workaholic/deployments/docs-site.md` is the full procedure. Which commit a hostname
carries is a `curl https://<host>/version.json` away — every deploy stamps it. Staging is
publicly reachable and refuses crawlers; production is indexable.

Also at the root: `plugins/qfs/` (the Claude Code / Codex plugin and its generated skills),
`containers/` (two turnkey live-round boxes), `deploy/dev/` (a Postgres + MariaDB dev stack for
live SQL work), `scripts/` (repository-wide scripts — today `stamp-docs-deploy.sh`, which both
publish jobs call), and `.workaholic/` (the engineering queue — tickets, missions, stories,
feedback).

### How qfs-viewer got here

`packages/qfs-viewer/` was imported as a **snapshot** from the standalone, private
`qmu/qfs-viewer` repository. The import kept the two halves apart on purpose:

- its **live** queue merged into the root `.workaholic/` (todo tickets, active missions), so one
  queue drives both projects;
- its **history** sits unchanged at `packages/qfs-viewer/.workaholic/`, read as an archive.

Since 2026-07-18 this repository is the development base for qfs-related work, so the upstream
repository is history rather than a second place to commit. The snapshot is why the package still
carries its own `CLAUDE.md`, its own ADRs, and its own script runner instead of being folded into
the Rust workspace's conventions.

### The one seam between them

The coupling runs one way and is recorded in the viewer's own ADRs, not on the qfs side:

- **qfs is found, not bundled** (ADR 0009) — the viewer shells out to a `qfs` binary it locates on
  `PATH`; it never vendors, links, or downloads one. `qfs --json run|describe` *is* the interface,
  because there is no library to link and the npm-only dependency contract would forbid one.
- **The corpus comes from qfs's markdown collection path** (ADR 0008), which retired the viewer's
  in-process indexer.

The adapter is `packages/qfs-viewer/packages/qfs-viewer/src/vendors/qfsRunner.ts`. It implements one
of the three planned issuance forms — an on-demand command invocation per query — and leaves a
locally running qfs server and a remote qfs as typed skeletons that answer every query with an error
naming themselves.

## `packages/qfs/` — the product

The Cargo workspace root. Members are `crates/*` (48), `spikes/*` (one throwaway parser comparison,
`publish = false`), and `xtask`. What the crates are and how a statement travels through them is
[the architecture as built](/guide/architecture); this section is only the tree a contributor meets.

| Path | What it is |
| --- | --- |
| `crates/` | The 48 production crates, including the `qfs` binary crate that is also the composition root |
| `xtask/` | The cargo-xtask build tool: `gen-docs`, `gen-skills`, `check-migrations`, `dist`. `publish = false`, never shipped in the binary. Run as `cargo run -p xtask -- <cmd>` |
| `fixtures/`, `crates/*/fixtures/` | `.qfs` documents and data used by tests and by the declared-driver corpus |
| `deploy/` | `qfs.service`, the project-local systemd unit template (never installed system-wide). Its `KillSignal=SIGTERM` is the graceful-drain claim `crates/cmd/tests/e2e_serve.rs` pins. Release artifacts are **not** built here — `xtask dist` builds them, from `release.yml` |
| `scripts/check-no-live-credentials.sh` | The credential-shape gate the release job runs before publishing |
| `install.sh` | The end-user installer: detects OS and arch, downloads the matching release tarball, **verifies its sha256**, installs the binary. Note the path — it lives here, not at the repository root |
| `rust-toolchain.toml`, `rustfmt.toml` | The pinned toolchain and format config; `rustup show` installs from the file |

## `packages/qfs-viewer/` — the imported package

Two npm packages under one tree, plus its own documentation and workloads.

| Path | What it is |
| --- | --- |
| `packages/qfs-viewer/` | The publishable package (`qfs-viewer@0.0.1`). `bin/qfs-viewer.mjs` is the launcher; `src/` is laid out as `domain/{model,usecase}` + `entrypoints/` + `vendors/` + `testkit/` (ADR 0004) |
| `packages/plggmatic/` | The plggmatic UI engine (`plggmatic@0.2.0`), ported here when `qmu/plggmatic` was retired. Consumed **from the npm registry** like every other dependency, not by a `file:` link — a `file:` link would break the npx smoke |
| `docs/adr/` | Ten ADRs plus an index: the recorded reasoning. `docs/plggmatic-semantics/` holds the frozen flow-DSL spec and its PoC findings |
| `workloads/development/` | One container that installs the package and serves **this repository's own corpus** with the tree bind-mounted, so editing markdown on the host hot-reloads the index. `./scripts/serve-development.sh` → `localhost:4100` |
| `scripts/` | The gate runner and its parts (below), plus `npm-install.sh`, `format.sh` (Prettier, per-package `printWidth: 50`), and `plgg-tool.sh` |

**How it runs.** At a repository root:

```sh
npx qfs-viewer serve [--port <n>] [--read-only]   # scan this repository and serve it
npx qfs-viewer mcp                                # serve it to an agent over MCP (stdio)
```

`serve` scans the corpus, holds it in an immutable on-memory index, hot-reloads as the tree changes,
and serves it three ways: the column browser at `/`, addressed trails at `/resolve/<trail>` (the
canonical address — ADR 0007 makes it subsume the older `?cols=`), and JSON at `/api/health`,
`/api/errors`, `/api/documents` and `/api/documents/<id>`. `--read-only` serves the corpus without
the authority to change it. The MCP face exposes four tools: `list_documents`, `get_document`,
`list_tag_groups`, `corpus_health`.

**What it depends on.** `plgg`, `plgg-md`, `plgg-mcp`, `plgg-server`, `plgg-view` and `plggmatic`,
all from the npm registry as published `^version` ranges, and nothing else at runtime. Dev
dependencies (`typescript`, `plgg-bundle`, `plgg-test`, `@types/node`) are exempt from that contract,
because it governs what ships at runtime and no package could build or test without them.

**`errorCount` is not expected to be zero.** `/api/health` reports the documents indexed and the
front matter rejected; a rejected document is still indexed and served with its front matter as
`None`, and `/api/errors` names each one with its line. The count tracks the markdown in the tree,
so it moves whenever a ticket or story is written. Read `/api/errors` before treating a count as a
fault: the subset that rejects `&` aliases, `!!` tags, merge keys and `|`/`>` block scalars is
fail-closed on purpose.

## The verification surface

Two gate families, one per project, each run from its own directory. Nothing runs both.

### `packages/qfs/` — the Rust gate

```sh
cd packages/qfs
cargo build --workspace
cargo test --workspace
env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p xtask -- gen-docs --check
cargo run -p xtask -- gen-skills --check
cargo run -p xtask -- check-migrations
```

| Command | What it proves | What it alone catches |
| --- | --- | --- |
| `cargo test --workspace` | The suite, all of it hermetic — no network, no credentials, no sockets (the `qfs-test` harness institutionalizes that) | Two guards live only here: `crates/cmd/tests/dep_direction.rs` (no cycles, no back-edges, tokio confined to `qfs-runtime` and its leaves) and the docs-drift golden inside `qfs::docs`. Also `crates/cmd/tests/faq_cli_surface.rs`, which walks the real clap tree so a renamed flag the FAQ cites fails CI, and `crates/test/tests/roadmap_cookbook.rs`, which ratchets how much of the query cookbook parses today |
| `env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1` | That no `qfs` unit test resolves its config home from the ambient environment — every one of them opens an isolated `testenv::HomeGuard` home rather than the shared `$HOME/.config/qfs` | The `store.rs` `cfg(test)` guard that refuses the shared-home fallback, which the line above **cannot** fire reliably: `HomeGuard` sets `XDG_CONFIG_HOME` process-wide, so a test that forgot its guard passes whenever a guarded sibling is running concurrently, and an ambient `XDG_CONFIG_HOME` suppresses the guard independently. Serialised and with the variable unset, a missing guard fails every run instead of some runs (measured 2026-08-18: parallel green 3/3 with a guard deliberately removed, serialised red every time) |
| `cargo clippy --workspace --all-targets -- -D warnings` | The lint floor: `unsafe_code = forbid` workspace-wide, and `unwrap_used` / `expect_used` / `panic` denied in non-test library code | A panic path introduced into a library. **Never `--all-features`**: `qfs-host`'s `host-daemon` and `host-workers` features are mutually exclusive, so CI lints the two separately with `-p qfs-host --features <one>` |
| `cargo fmt --all --check` | Formatting, against the committed `rustfmt.toml` | — |
| `cargo build --workspace` | It compiles for the host | The cross-compile legs (CI adds two targets and a wasm32 build of the `qfs-host` core) catch what a host build cannot |
| the three `xtask` checks | Anti-drift; see below | — |

CI (`.github/workflows/ci.yml`) runs `fmt`, `clippy` (three invocations), `build + test` — whose
job also carries the serialised, `XDG`-unset re-run of the `qfs` lib suite as a second step — two
cross-compiles, a wasm32 host-core build, the docs site production build, and the viewer gate. It
does **not** invoke `xtask` at all, so of the three anti-drift checks only `gen-docs` is defended
automatically — by that docs-drift unit test, not by the command.

`ci.yml` is `on: push: branches: ["**"]` plus `pull_request`, and a **tag** push matches neither, so
nothing in it re-checks a tagged tree. That matters for one property only: `release.yml`'s
`docs-deploy-production` publishes the tag's `docs/` to `qfs.qmu.co.jp`, where a drifted reference
page would contradict the binary `install.sh` installs from the same tag. `release.yml` therefore
carries its own `docs-drift` job — `cargo run -p xtask -- gen-docs --check`, the command this time —
and the publish `needs` it. A drifted tag still publishes its GitHub Release (the binary is not at
fault) and leaves `qfs.qmu.co.jp` serving the previous version; the fix is to regenerate on `main`
and re-tag.

The `docs-build` job does double duty: on any branch it proves every documentation page compiles,
and on a push to `main` it also publishes the built site to `staging-qfs.qmu.co.jp`. The publish
steps are guarded by `github.event_name == 'push' && github.ref == 'refs/heads/main'`, so a topic
branch and a pull request build the site and publish nothing. Production is unreachable from this
workflow: it is published by `release.yml`'s `docs-deploy-production` job, on the `v*` tag, after
the GitHub Release succeeds.

### `packages/qfs-viewer/` — the TypeScript gate

```sh
cd packages/qfs-viewer
./scripts/npm-install.sh    # install every package's dependencies
./scripts/check-all.sh      # the one reproducible gate; must exit 0
```

`check-all.sh` is the whole contract — CI calls it and adds nothing. Its order is deliberate:

| Step | What it proves |
| --- | --- |
| `gate-dependencies.sh` | Runtime dependencies are plgg-family only. It **self-tests its own red/green logic first** — a gate never proven to fail is not a gate |
| `gate-vendor-boundary.sh` | Third-party imports (`node:*`, the tsc API, any bare non-plgg specifier) appear in production code only under `src/vendors/` or `src/entrypoints/`. Also self-tested, and a package exempted but clean is reported as a **stale** exemption |
| `build.sh` | Every package's dist builds, in dependency order |
| `smoke-npx.sh` | The headline promise: it packs the package as the registry would serve it, installs the tarball into a scratch tree so the bin really lives under `node_modules`, and runs it — for **every** runtime installed (node, bun, deno), skipping an absent one out loud. The unit suites execute TypeScript source, so a broken launcher or a wrong `files` list would otherwise ship green |
| `test-qfs-viewer.sh`, `test-plggmatic.sh` | `tsc --noEmit` plus the coverage-gated unit suite, per package |

The smoke's runtime matrix is why this gate is environment-sensitive in one direction: a machine
with bun or deno installed runs the packed bin under them too, and a failure there is a real failure
of the published artifact under that runtime — not of the change under test.

**What a green run of this gate actually proves, as of 2026-08-17.** CI's `viewer-check-all` job
installs Node 24 and nothing else, so the runtimes it exercises are narrower than a developer's, and
until this was written down neither the README nor `CLAUDE.md` said so:

| Runtime | In CI | Locally |
| --- | --- | --- |
| node | Installed — this is what a green CI run attests to | Proven |
| bun | Absent, so skipped out loud | **Broken upstream**, and reported as `NOT COVERED` under a dated exemption: bun 1.3.11 cannot parse `plgg-md`'s published dist — one regex class written with raw control characters that node accepts and bun rejects as "range out of order". Present in `plgg-md` 0.0.2 and 0.0.3, so no bump fixes it. Revisit after 2026-11-17; filing it against `qmu/plgg` is ticket `20260817131540` |
| deno | Absent, so skipped out loud | Unproven — absent from the container this was measured on |

The exemption is deliberately narrow: it matches that one error signature in that one dependency, does
not count bun as covered, and any other bun failure still fails the gate. Dropping bun from the loop
instead was rejected — `smoke-npx.sh`'s own comments record that a silent skip is how bun stayed broken
for a whole session.

## The three anti-drift generators

Each owns a set of files that must never be hand-edited: edit the source, run the generator. All
three are `xtask` subcommands, run from `packages/qfs`.

| Generator | Owns | Source of truth | Why never hand-edited |
| --- | --- | --- | --- |
| `cargo run -p xtask -- gen-docs` | `docs/language.md`, `docs/drivers.md`, `docs/server.md` | The binary's own registries: the frozen reserved-keyword set, the cred-free compiled describe registry, the server binding forms | A hand-edited reference can claim a keyword, a column or a verb the binary does not have. Fix the prose in `crates/qfs/src/docs.rs` and regenerate. Enforced automatically twice: the docs-drift golden test inside `qfs::docs` makes `cargo test --workspace` (and so CI's `build-test`) fail on drift for a branch or PR, and `release.yml`'s `docs-drift` job runs `gen-docs --check` on a `v*` tag before the production docs publish is allowed to run |
| `cargo run -p xtask -- gen-skills` | The 14 `plugins/qfs/skills/*/SKILL.md`, plus the `.claude/skills/<name>` symlinks | `docs/cookbook/*.md` — each article carries `skill_name` + `skill_description` front matter, and the skill is that front matter plus the article body verbatim | A skill is what an agent loads; a hand-edited one drifts from the article a human maintains. **Not enforced by any test or CI step** — `--check` catches it only when someone runs it |
| `cargo run -p xtask -- check-migrations` | Nothing — it is a guard, not a writer | `crates/store/src/schema/*.sql` versus their content at the last release tag | An already-shipped migration body edited in place would leave existing installations with a recorded checksum that no longer matches, and the runtime heal path cannot fire on a fresh CI database. Changing a shipped body needs an audited `SUPERSEDED_BODIES` entry. **It needs release tags**: with none reachable it returns clean rather than failing, so a shallow clone or a fork without tags cannot verify this gate |

**Re-version the plugin when a shipped change touches a CLI surface the skills mention.** The plugin
carries its own version, independent of the binary's `v0.0.x`, in four places:
`plugins/qfs/.claude-plugin/plugin.json`, `plugins/qfs/.codex-plugin/plugin.json`, and both `version`
fields in the repository-root `.claude-plugin/marketplace.json` — the marketplace manifest is at the
root, beside `.agents/plugins/marketplace.json`, which carries no version and needs no bump.
Regenerated skills only reach installed caches when that version moves — a stale cache keeps
teaching retired commands — so a taught-surface break bumps the minor and anything else
skill-affecting bumps the patch, in the same change. All four read `0.20.0` at this commit.

## Version and release

There is no server deployment. **The deliverable is a published GitHub Release.**

1. **Every shipped change bumps the patch** in `packages/qfs/crates/qfs/Cargo.toml` before its pull
   request opens. The conceptual SemVer policy — the versioned surface is the grammar plus the
   registries — is in the README; this operational rule applies regardless.
2. **The tag is cut after the merge** and must match:
   `git tag -a vX.Y.Z -m "qfs vX.Y.Z" && git push origin vX.Y.Z`, so the published release and `qfs --version` agree. `qfs --version` prints the
   semver, the git sha and the target triple, all baked in by `build.rs`.
3. **`.github/workflows/release.yml` fires on the `v*` tag.** One job per target builds a tarball on
   a runner that can link it — `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
   `x86_64-apple-darwin`, `aarch64-apple-darwin` — each via
   `cargo run -p xtask -- dist --target <triple>`, which builds, strips, tars, and writes a `<tarball>.sha256`. `dist` refuses to run
   unless `QFS_DIST_ALLOW=1` is set, which CI sets and a local machine does not: the release build
   wedges a constrained disk.
4. **The publish job** runs the credential-shape gate (`packages/qfs/scripts/check-no-live-credentials.sh`)
   before anything is uploaded, then publishes every artifact to a GitHub Release with generated
   notes, failing on an unmatched file.
5. **`packages/qfs/install.sh`** is what a user runs. It detects OS and architecture, downloads the
   matching tarball, verifies its sha256, and installs the binary.

**The Cloudflare Workers wasm artifact is parked.** There is no cdylib entrypoint, so releases ship
the four native binaries only. The `qfs-host` crate's `host-workers` feature compiles and its pure
cores are wasm-clean — CI builds them for `wasm32-unknown-unknown` — but the full-binary wasm build
is not part of a release. Anything a page calls parked is parked, not shipped.

## Maintaining this page

Nothing generates or checks it — it is a dated reading of the repository, like
[the architecture as built](/guide/architecture) and [the documentation map](/documentation-map).
Re-take it by running each gate command from the directory named here, re-reading
`.github/workflows/{ci,release}.yml` and `packages/qfs/xtask/src/main.rs`, and replacing the commit
and date in the header.
