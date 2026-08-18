# The architecture as built

How the shipped `qfs` binary is put together: the crates, the path a statement travels, where state
lives, and the faces the binary serves.

**Read from the source at commit `52b0410` (`origin/main`), 2026-08-17, binary `qfs 0.0.108`.**

This page describes what the code does. It is not design: where the source and
[the blueprint](/blueprint) disagree, the source is recorded here and the disagreement is named
rather than resolved — see *Where this page and the blueprint differ*.

**Its boundary with [the current design snapshot](/guide/design-snapshot).** That page is the
operating model an *operator* meets: paths, mounts, accounts, `/sys`, the safety loop, dump and
restore. This page is the system a *contributor* meets: crate boundaries, the query path, the
stores' files on disk, the entry point of each face. Neither repeats the other; when an operator
concept needs a name here, this page links there instead of re-explaining it.

## The workspace

48 crates under `packages/qfs/crates/`, plus the `xtask` build tool and one throwaway
`spikes/parser-spike` (`publish = false`, kept in the workspace so it builds and lints with the
rest). The layering below is read from `[dependencies]` in each crate's `Cargo.toml`, not from the
crate names, and it is enforced mechanically by `crates/cmd/tests/dep_direction.rs` (a
`cargo metadata` test): no cycles, no back-edges, and tokio confined.

### Pure leaves — no workspace dependencies, or only other leaves

| Crate | Owns |
| --- | --- |
| `types` | The canonical row and schema model: `Value`/`Row`/`RowBatch`, `Schema`/`ColumnType`, schema algebra, typed predicates, `DriverId`. No workspace deps at all |
| `lang` | The frozen reserved-keyword closed core. A new backend adds zero keywords; a freeze test locks the set |
| `crypto-core` | The dependency-free crypto primitives every credential path shares (hashing, sealing) |
| `parser` | The parser front door: `parse_statement` → the owned `Statement` AST. Depends on `lang` + `types`; the parser library's own types never appear in its public API |
| `plan` | Effects-as-data: the typed `Plan` DAG of `EffectNode`s, the `PlanApplier` seam, `commit`, and `PREVIEW` rendering. Pure |
| `txn` | The transactional correctness envelope: `EffectKey` idempotency, `@version`/ETag preconditions, `CommitStrategy`, saga/ACID executors, the audit ledger. Pure orchestration over `plan` + `types`; carries no tokio |
| `sql-core` | The dialect-agnostic, injection-safe qfs-query → parameterized-SQL core (`Dialect`, the emitter, the pure compiler, catalog DTOs). Single-sourced so both `driver-sql` and `driver-cf` reuse one emitter |
| `http-core` | The vendor-free HTTP exchange DTOs and the single header-redaction authority (`SENSITIVE_HEADERS`, the redacting `Debug` impls). Carries no `reqwest`, no tokio |
| `secrets` | The credential/secret store surface and multi-connection resolution. Over `types` only |
| `identity` | The identity domain: the operator and members of a host |
| `session` | Server-side sessions over `identity` + `secrets` + `crypto-core` |
| `oauth` | The OAuth 2.1 authorization-server domain |
| `google-auth` | The shared Google OAuth2 + multi-account base: the loopback authorization-code flow, token exchange/refresh, per-account refresh-token storage, and the authenticated `GoogleApiClient` the Gmail, Drive and Analytics drivers ride. Off the runtime by design — network rides a local, runtime-free `HttpExchange` seam |
| `tunnel` | The agent-fabric transport protocol core |
| `store` | The embedded SQLite persistence substrate: the two databases, the embedded migration bodies, and the migration ledger |

### The contract layer

| Crate | Owns |
| --- | --- |
| `driver` | The `Driver` contract every backend implements: `mount()`, `id()`, archetype, typed `Schema`, capabilities and the parse-time gate, `ProcSig`/pushdown/prelude/`@version`, and the `applier()` seam. Also owns `CfsError` and `Path` (decision D1) |
| `codec` | The pure `bytes ↔ rows` `Codec` contract — the `DECODE`/`ENCODE` formats, one workspace-wide registry rather than a per-driver list |

### Planning and evaluation

| Crate | Owns |
| --- | --- |
| `pushdown` | The pushdown planner: `LogicalPlan`/`PhysicalPlan`/`ScanNode`/`PushedQuery`, AST lowering, `partition_by_source` (the source split that makes federation possible), and `explain()`. Pure, no I/O |
| `engine` | The local combine engine: the `CombineEngine` seam and the in-house `MiniEvaluator` that runs the cross-source residual — filter, project, hash join, set ops, aggregate, sort, limit, `EXPAND`, and the `transform` stage. Dependency-light and wasm-clean |
| `core` | The shared engine glue: the three registries (`MountRegistry` with longest-mount-prefix routing, `ProcRegistry`, `CodecRegistry`), `Engine`/`Session`, the `Evaluator` (resolve → capability gate → typecheck → fold to a `Plan` or a relation), the server DDL desugar, and the `plan_query` seam into `pushdown` |

### Execution

| Crate | Owns |
| --- | --- |
| `exec` | The integration layer above the spine: the end-to-end read executor, the one-shot orchestration (statement source, addressing, the PREVIEW/COMMIT gate, output rendering, the exit-code contract), the interactive shell's logic, the codec tail, and the declared-driver evaluation. Owns its own async `ReadDriver` read seam — deliberately **not** a `runtime` consumer |
| `runtime` | The async effect interpreter: `Interpreter::commit`, `ApplyDriver`/`DriverRegistry`, the `EffectError` taxonomy, the `CapabilitySet` gate, and the sync→async `PlanApplierBridge`. **tokio is confined here**; nothing in the pure spine depends back onto it |

### Drivers — 16 crates over one contract

Each is a **leaf** runtime consumer: it bridges its synchronous `PlanApplier` to the async
`ApplyDriver`, so tokio dead-ends in the driver and never re-enters the spine. None depends on
another.

| Crate | Mount | Owns |
| --- | --- | --- |
| `driver-local` | `/local` | The first concrete driver: a blob/namespace over the host filesystem, least-privilege sandbox, atomic temp+rename writes, copy→verify→delete |
| `driver-fs` | `/fs` | The first-class filesystem driver over a root allowlist (empty, i.e. deny-all, in the cred-free describe registry) |
| `driver-gmail` | `/mail` | The mailbox as an append log: labels as directories, messages as files. `REMOVE` = trash (the client trait has no permanent delete); `CALL mail.send` is the irreversible transition |
| `driver-gdrive` | `/drive` | Drive as a blob namespace: My Drive and shared drives, Google-native exports, folders, copy, trash |
| `driver-ga` | `/google-analytics` | Google Analytics 4, read-only relational |
| `driver-github` | `/github` | The GitHub object graph plus workflow: PRs and issues as rows, `CALL github.merge` behind the irreversible gate |
| `driver-git` | `/git` | The versioned tree and history — one driver proving a path can carry a `@ref` coordinate |
| `driver-sql` | `/sql` | Postgres, MySQL and SQLite over the shared `sql-core` emitter; every value bound as a parameter |
| `driver-cf` | `/cf` | Cloudflare: queue PULL and Artifacts (D1 and KV moved onto the committed `cloudflare.qfs` declaration) |
| `driver-objstore` | `/s3`, `/r2` | S3-compatible object storage |
| `driver-http` | `/rest` | The generic HTTP/REST escape hatch, and the transport declared drivers ride |
| `driver-sys` | `/sys` | The administration surface: paths, accounts, settings, policies, drivers, billing, audit as queryable rows |
| `driver-transform` | `/transform` | The model-calling pipe stage's definitions and providers |
| `driver-type` | `/type` | The declared-type catalog |
| `driver-directory` | — | The identity directory |
| `driver-claude` | `/hosts/<host>/claude` | Claude Code sessions as queryable, steerable rows |

### The faces and the composition root

| Crate | Owns |
| --- | --- |
| `cmd` | argv parsing (clap-derive) and dispatch. Deliberately logic-free: it may depend only on `core`, `server`, `exec` and `pushdown`, and takes injected launchers for everything that needs a driver |
| `server` | The server face: the binding registry and the reconcile seam behind `/server` |
| `http` | The HTTP serving binding — the listener that turns a boot config into a served surface |
| `mcp` | The MCP serving binding: the JSON-RPC / Model Context Protocol face, pure protocol and tool logic with the live half injected |
| `watchtower` | The event bus and watchers behind webhook and trigger bindings |
| `host` | The deployment host-adapter seam. Its two features are **mutually exclusive** — `host-daemon` (live) and `host-workers` (parked) — which is why the clippy gate is `--all-targets` and never `--all-features` |
| `provision` | The declarative `qfs plan` / `qfs apply` fetch → diff → reconcile core |
| `skill` | The agent operating procedure embedded into the binary (`qfs skill`) and the golden example corpus that proves each example parses, evaluates, and matches a checked-in PREVIEW |
| `qfs` | The single binary **and** the composition root: it is the one allowlisted crate that may depend on every concrete driver, so it builds the describe registry, the shell, the serve stack, the MCP engine, the commit registry, and the store openers, and injects them downward. `main.rs` is thin — it forwards argv to `qfs_cmd::run` |
| `test` | The dev-dependency-only offline harness: no credentials, no sockets. Not shipped in the binary |

### The spine

Arrows point toward more-foundational crates.

```
qfs (bin, composition root)
 ├─ cmd ─────────────────► core ──► { lang, parser, plan, driver, codec, types, pushdown, secrets }
 ├─ exec ────────────────► { core, parser, pushdown, engine }
 ├─ http / mcp / watchtower ──► { server, exec, core }
 ├─ provision ───────────► { core, server, parser }
 ├─ host ────────────────► server
 └─ 16 driver-* leaves ──► { driver, plan, types, codec, runtime, … }

pushdown ──► { driver, types, plan, parser }        engine ──► { pushdown, types, driver, parser }
runtime ───► { plan, types, txn }                   txn ────► { plan, types }
store ─────► { crypto-core, identity, session, secrets }
types ─────► (serde only — the lowest node)
```

**tokio confinement.** `runtime` is the sole impure stage, and the guard has three parts: its own
workspace deps are pinned to `{plan, types, txn}`; **every** crate depending on it must be a leaf, so
a new driver needs no test edit; and a small identity allowlist pins the intent so an unintended new
runtime consumer is caught even if it happens to be a leaf today. `exec` is the notable non-consumer
— it carries its own async read seam rather than taking the runtime edge, because the runtime's write
`ApplyDriver` returns affected counts and never rows.

## How a read travels

`qfs run '/mail/inbox |> WHERE is_unread |> SELECT subject |> LIMIT 5'`

| Stage | Entry point |
| --- | --- |
| 1. argv → dispatch | `qfs::main` → `qfs_cmd::run` |
| 2. one-shot orchestration | `qfs_exec::run_oneshot` — resolves the statement source, validates addressing, chooses the renderer |
| 3. parse | `qfs_exec::parse` → `qfs_parser::parse_statement` → `Statement` |
| 4. plan | `qfs_core::plan_query(stmt, mounts)` — lowers the AST to a `LogicalPlan`, then `qfs_pushdown::partition_by_source` splits it per source into a `PhysicalPlan` of `ScanNode`s with their `PushedQuery` |
| 5. scan | `qfs_exec::execute_read` walks `PhysicalPlan::scans()` in plan order and calls `ReadDriver::scan` per leaf, awaiting each |
| 6. re-check the pushed predicate | `qfs_engine::apply_where`, unless the facet declares `honors_pushed_filter`. A pushed `WHERE` is a narrowing hint, never a delegation of correctness — a facet that ignores it over-returns rather than answering an unfiltered relation at exit 0 |
| 7. combine | `qfs_engine::MiniEvaluator`'s `CombineEngine::execute(&physical, ScanResults::new(batches))` folds the residual ops — filter, project, join, limit, aggregate, `transform` |
| 8. codec tail | `qfs_exec::apply_codecs` runs any trailing `DECODE`/`ENCODE` locally; the planner drops them on purpose, because a decode produces a data-dependent schema it cannot know |
| 9. render | `qfs_exec::TableRenderer` on a TTY, `JsonRenderer` when piped |

A read never builds an effect plan: `build_plan` returns `Plan::pure()` for a `SELECT`. The serve
path needs a read's subjects before any scan runs, so `qfs_exec::scan_targets` derives them from the
same physical plan step 5 executes — the policy gate can never adjudicate a different set of paths
than the driver is then asked for.

## How a write travels

`qfs run "UPSERT INTO /local/out.csv …" --commit`

| Stage | Entry point |
| --- | --- |
| 1–3. argv → parse | as above, through `qfs_exec::run_oneshot` |
| 4. resolve and gate | `qfs_core::Evaluator::eval` runs `Resolver::resolve_statement` **first**, so a denied verb, an unknown procedure or an unbound name fails before a plan exists |
| 5. typecheck | the same call runs the static primitive checker (`qfs_core::typeck::check_expr`) at plan time, wired through `Evaluator::with_stdlib` in `qfs_exec::build_plan` — a mismatched predicate or a built-in handed the wrong type is a structured plan-time error, so a type-failing plan never reaches commit |
| 6. build the plan | `qfs_exec::build_plan` → `EvalValue::Plan(plan)`: effects as data, nothing applied |
| 7. PREVIEW (the default) | `qfs_exec::plan_preview` → `qfs_plan::preview`. Preview is a plan projection, not an apply dry-run |
| 8. COMMIT | `qfs_exec::apply_via` dispatches to the injected `WorldApply`, which in the binary is `qfs::commit::apply_plan_rooted` — it builds a current-thread tokio runtime and drives `qfs_runtime::Interpreter::commit(plan, &CapabilitySet)` over the live `DriverRegistry` |
| 9. apply per leg | the interpreter calls each driver's `ApplyDriver`, which is that driver's synchronous `PlanApplier` bridged by `qfs_runtime::PlanApplierBridge`. Transactional legs ride the `qfs_txn` envelope: `EffectKey` idempotency, `@version` preconditions, the chosen `CommitStrategy` |

Two gates sit on this path and neither is a formatting difference. An **irreversible** effect (a
`REMOVE`, a `CALL mail.send`) fails closed in a non-interactive one-shot unless
`--commit-irreversible` is passed, because there is no TTY to confirm on. And `local_root` — the
`/local` root the launch context planned against — must be the same root the commit applies under: a
preview resolved under one root and a commit applied under another is a mis-targeted write. The
one-shot, job and server contexts root at `/`; an interactive session roots at its cwd on both faces.

## Where state lives

Two SQLite databases plus one encrypted credential file, all under the same directory:
`$XDG_CONFIG_HOME/qfs/` when that variable is set and non-empty, otherwise `~/.config/qfs/`.

| File | Crate that owns the schema | Holds |
| --- | --- | --- |
| `system.db` | `store` (`SYSTEM_MIGRATIONS`) | Host and operator administration: identity, sessions, invites, hosts, settings, policies, declared drivers and their pushdown, transforms, billing, OAuth clients and keys, OIDC providers, user keys, the config registry, the DDL/config event trail, and audit metadata |
| `project.db` | `store` (`PROJECT_MIGRATIONS`) | Project-local bindings and credential *references*: path bindings, shared and broker connections, mount coordinates, account consent and its secret refs, Google app labels, rotation state, vault key slots, secrets metadata, e2e state |
| `credentials` | `secrets` (`default_credentials_path`) | The encrypted vault: token values and OAuth client secrets. Never in a dump, never in a log |

32 embedded migration bodies live in `crates/store/src/schema/*.sql` (19 system, 13 project),
`include_str!`-ed into those two constants and applied on open with a recorded checksum ledger. The
binary is the only crate that resolves a real file path — `qfs::store::default_system_db_path` and
`default_project_db_path` — because `store` is a leaf and nothing in the spine may name a file.
Opening the databases and migrating them is start-time infrastructure, not a qfs effect plan: it
never goes through preview/commit; it is the substrate that later `/sys` writes preview and commit
*over*.

The checksum ledger is why `check-migrations` exists: the runtime mismatch/heal path only fires
against a database that already recorded the old checksum, so nothing would catch a shipped
`schema/*.sql` body edited in place before a merge. `cargo run -p xtask -- check-migrations` diffs
each body against its content at the last release tag and fails without a matching audited
heal-forward entry.

## The faces

One engine; every face reaches it through the same describe → preview → commit path, and none has a
privileged shortcut.

| Face | Entry point | Notes |
| --- | --- | --- |
| CLI one-shot | `qfs_cmd::run` → `qfs_exec::run_oneshot` | 21 subcommands; see the [CLI reference](/guide/cli) |
| Interactive shell | `qfs::shell::run_interactive_shell` | Started by `qfs` with no subcommand. The logic is `qfs_exec::shell` (resolve, desugar, `eval_line`, completion); the binary owns only what a real terminal needs — line reader, history, prompt redraw |
| HTTP listener | `qfs::serve::run_serve` → `qfs_http::serve_config` | `qfs serve <config.qfs>`; serves the `endpoint` / `webhook` bindings, bounded to 10 000 result rows by default |
| MCP | `qfs::mcp::ServeMcpEngine`, composed into the same listener at `POST /mcp` | The protocol and tool logic stay pure in `qfs-mcp`; the live half (describe registry, real `build_plan`, runtime-backed apply, redacted connection list) is injected by the binary. Its commits run off the serve runtime on a dedicated thread, since `block_on` panics on a runtime thread |
| Embedded dashboard | `qfs::dashboard::serve_dashboard` | A static SPA compiled into the binary and served over loopback, plus a thin JSON bridge into the same engine path. The commit bridge is locked unless the request arrives with bearer material on a permitted address |
| Console delivery | `qfs::console::deliver` | Fetch → verify integrity hash → cache → serve same-origin, so the browser never touches a third-party origin. Present in the binary as delivery machinery; the serve route table today wires the dashboard, not the console |
| Host adapters | `qfs_host`, feature-gated | `host-daemon` is the live target; `host-workers` is parked |

## Where this page and the blueprint differ

Read the blueprint's per-section status marker before treating any of its statements as current
fact; this page names the three places where the marker and the source do not line up. None is
resolved here — that is the blueprint's own to change.

1. **§14 (the console face) is marked `blueprint`, and part of it is in the binary.**
   `crates/qfs/src/console.rs` implements the section's delivery contract — fetch, verify, cache,
   serve same-origin — while the served browser face today is the embedded dashboard the section
   says the console absorbs "at parity". So the marker is right that the console is not the shipped
   screen, and wrong to imply none of it exists.
2. **§19 (agents) is marked `blueprint`, and the CLI ships `qfs agent run`.** The marker's own
   parenthetical says the grammar, subject, functions and cadence are "being built", so this is a
   mixed state rather than a contradiction — but a reader taking `blueprint` at face value would
   miss a shipped subcommand with a live policy gate under the agent's own subject.
3. **`packages/qfs/ARCHITECTURE.md` contradicts the workspace, not the blueprint.** It maps 20
   crates against the 48 on disk, predates the whole `qfs-exec` integration layer, and its lints
   section still names `clippy --all-features`, which the `qfs-host` feature exclusivity now
   forbids. Its boundary rules and decisions D1/D2 remain accurate.

## Maintaining this page

Nothing generates or checks it — it is a dated reading of the source, like
[the documentation map](/documentation-map). Re-take it the way it was taken: the crate table from
each `Cargo.toml`'s `[dependencies]`, the traces by following the named symbols, the stores from
`crates/store/src/schema/` and `qfs::store`, the faces from the binary's composition roots. Then
replace the commit and date in the header.
