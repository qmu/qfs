# cfs architecture (Rust workspace)

This document records the **crate-boundary rules** of the `cfs` Rust rebuild. It is
the durable companion to [`RFD-0001`](.workaholic/RFDs/0001-cfs-architecture.md) (the
single source of truth) and to the E0 trip artifacts under
`.workaholic/trips/cfs-foundation-e0/`. Every later ticket must add code **inside**
these boundaries without restructuring the workspace.

> Note: the legacy Go `gmail-ftp` program (`main.go`, `internal/`, `go.mod`) lives
> alongside this workspace and is untouched. Per RFD §0 it is *subsumed as a future
> driver*, not merged. The Rust workspace coexists with it.

## Crate map

| Crate (`crates/<dir>`) | package | Role (RFD §) |
|---|---|---|
| `cfs` (bin) | `cfs` | The single binary; thin `main.rs` calling `cfs_cmd::run` (§9). |
| `cmd` | `cfs-cmd` | argv parsing (clap-derive), dispatch into the engine; **no domain logic** (§7). |
| `core` | `cfs-core` | Shared engine glue: 3 registries, `Engine`/`Session`, re-exports, `CfsError` (§3/§6). |
| `lang` | `cfs-lang` | The frozen reserved-keyword closed core (§3); AST lands here in E1. |
| `plan` | `cfs-plan` | Effects-as-data: the typed `Plan` DAG of `EffectNode`s, `PlanApplier`/`commit`, and `PREVIEW` rendering (§3/§6/§10). Depends on `cfs-types` (leaf) for the row model (t09). |
| `driver` | `cfs-driver` | The `Driver` contract (mount + `id()` plan identity, archetype, typed `Schema`, capabilities + parse-time gate, `ProcSig`/pushdown/prelude/`@version`, the `applier()` seam) + owned DTOs (incl. the out-of-crate `Capabilities` builder); owns `CfsError` & `Path` (§5/§9). |
| `codec` | `cfs-codec` | The pure `bytes ↔ rows` `Codec` contract (§4). |
| `types` | `cfs-types` | The canonical type & schema model: `Value`/`Row`/`RowBatch`, `Schema`/`ColumnType`, schema algebra + typed predicates (§4/§5). **Leaf** crate (t05). |
| `server` | `cfs-server` | The server face: `serve` stub + `/server` mount seam (§8). |
| `parser` | `cfs-parser` | Parser front door skeleton; **filled by t02** (§2.2/§9). |
| `pushdown` | `cfs-pushdown` | The pushdown planner (t14, Domain): `LogicalPlan`/`PhysicalPlan`/`ScanNode`/`PushedQuery`, `partition_by_source` (source-split + federation), AST→IR lowering (predicates sourced from the AST, O-t07-3), and `explain()`. Pure; no I/O (§6). |
| `engine` | `cfs-engine` | The local combine engine (t14, Infrastructure): the `CombineEngine` seam + in-house `MiniEvaluator` running the cross-source residual (filter/project/hash-join/set-ops/aggregate/sort/limit/EXPAND). DuckDB rejected, ADR-0002; dependency-light + wasm-clean (§6/§9). |
| `txn` | `cfs-txn` | The transactional correctness envelope (t11): `EffectKey` idempotency, `@version`/ETag preconditions, `CommitStrategy`, saga/ACID executors, audit ledger, `RecoveryReport`. **Pure orchestration** over `cfs-plan`/`cfs-types` — carries **no** tokio of its own (§6). |
| `runtime` | `cfs-runtime` | The async effect interpreter (t10): `Interpreter::commit`, `ApplyDriver`/`DriverRegistry`, `EffectError` taxonomy, `CapabilitySet` gate, the sync→async `PlanApplierBridge`/`SharedApplier`. **tokio is CONFINED here** — the sole impure stage; nothing in the pure spine depends back onto it (§3/§5/§6). |
| `driver-local` | `cfs-driver-local` | The **first concrete driver** (t16): a blob/namespace driver over the host filesystem mounted at `/local` (`ls cp mv rm` + `upsert`/`remove`), least-privilege sandbox, streaming reads, atomic temp+rename writes, copy→verify(size+hash)→[delete]. A **LEAF runtime consumer** — it bridges its synchronous `PlanApplier` to the async `ApplyDriver`; tokio dead-ends here (§5/§6/§10). |
| `secrets` | `cfs-secrets` | The credential / secret store + multi-account resolution (t27): consumer-side, owned-DTO `Secrets` surface reusing the canonical `cfs_types::DriverId`. Depends on `cfs-types` only; `cfs-core` threads a `Secrets` handle into the driver-bind context (§10). |
| `sql-core` | `cfs-sql-core` | The **pure-leaf SQL compile/emit core** (t23 extraction from t17): the dialect-agnostic, **injection-safe** cfs-query → parameterized-SQL machinery (`Dialect` quoting/placeholders/upsert, the `SelectPlan`/`DmlOp` emitter that binds **every** value as a parameter, the pure `compile` query→plan compiler with a truthful residual) and the owned catalog DTOs (`Catalog`/`TableCatalog`/`ColumnDef`) + `SqlError`. The **single source of truth** for the sqlite emitter that **both** `cfs-driver-sql` (postgres/mysql/sqlite, t17) and `cfs-driver-cf` (Cloudflare D1, SQLite-over-HTTP, t23) reuse — so neither runtime-leaf driver depends on the other (the same single-source pattern as `cfs-http-core`). A pure leaf: its only workspace dep is `cfs-types`; carries **no** runtime/secrets/driver coupling. The runtime/secrets `From` adapters for `SqlError` live in the consuming driver crates (orphan-rule-safe explicit converters), not here (§5/§6). |
| `http-core` | `cfs-http-core` | The shared **pure-leaf** HTTP DTO + redaction crate (t19 refinement): the **single source of truth** for the vendor-free HTTP exchange DTOs (`HttpMethod` (the `#[non_exhaustive]` 4-variant set), `HttpRequest`, `HttpResponse`) and the header-redaction authority (`SENSITIVE_HEADERS` / `is_sensitive_header` + the redacting `Debug` impls). Carries **no** `reqwest`/`tokio`/`cfs-runtime` — its only workspace dep is `cfs-secrets` (the one `REDACTED` marker), so the closure is `cfs-http-core → cfs-secrets → cfs-types`. Both `cfs-driver-http` (real reqwest `HttpClient`) and `cfs-google-auth` (local `HttpExchange` seam) depend on it for ONE redaction set instead of each hand-copying a drift-prone duplicate (the prior hazard: a sensitive header added on one side and lagging on the other leaks a header *value* across the seam). The concrete transports stay in their own crates (§5/§10). |
| `google-auth` | `cfs-google-auth` | The **shared Google OAuth2 + multi-account auth base** (t19): the loopback authorization-code flow (`localhost`-not-`127.0.0.1` redirect — the load-bearing detail), access-token exchange/refresh, per-account refresh-token storage via `cfs-secrets` (keyed `(google, <encoded-email>)`), profile-email account identity, and the reusable `TokenSource` + authenticated `GoogleApiClient` (bearer inject, refresh-on-401) the Gmail (t20)/Drive (t21)/Analytics (t41) drivers build on. Scope-agnostic; tokens are `cfs_secrets::Secret`. Among workspace crates it depends on **`cfs-secrets` + `cfs-http-core` only** (both pure leaves reaching no further than `cfs-types`) — network rides a thin, synchronous, runtime-free `HttpExchange` seam (over the shared `cfs-http-core` DTOs) kept **local** so this crate does NOT depend on `cfs-driver-http` (a runtime leaf); the consuming drivers adapt their `Arc<dyn cfs_driver_http::HttpClient>` to `HttpExchange`. The HTTP DTOs + header redaction now come from the shared `cfs-http-core` leaf (single source of truth), not a local copy. Constructs no `Plan`/effect; `authorize` is native-only (feature-gated off `wasm32`) (§5/§6/§10). |
| `driver-gmail` | `cfs-driver-gmail` | The **first real `Driver`** (t20, RFD §5): subsumes the legacy Go `gmail-ftp` as one mount at **`/mail`** under the uniform VFS + pipe-SQL DSL, mapping the mailbox onto the **Append/log archetype** (labels = directories, messages = files, attachments = nested). Implements the t13 introspective contract (describe/capabilities/procedures/prelude/pushdown/applier) over the t19 `GoogleApiClient`. **Path-keyed least-privilege capabilities** (`/mail/drafts` = Insert\|Upsert\|Select\|Remove; `/mail/<label>` = Select\|Update\|Remove; a message = Select\|Remove; a thread = Remove only). SELECT pushes WHERE/LIMIT to the Gmail search `q=` with a **truthful local residual** — only `label`/`is_unread` map exactly; `from`/`to`/`subject` `=`/`~`/`LIKE` and the `date` bounds are looser Gmail operators pushed as a pre-filter and **re-checked locally** (over-fetch then filter, never wrong rows — RFD §6). **REMOVE = TRASH** (the `GmailClient` trait has no permanent-delete method at all); `CALL mail.send` (and the `SEND` prelude alias) is the `irreversible` transition (create-draft-then-send de-dupe). **Least-privilege scopes**: `gmail.modify` + `gmail.compose` only (no `mail.google.com`, no delete). **No vendor leak** — Gmail JSON decodes to owned DTOs at the mockable `GmailClient` boundary; `reqwest` stays in `cfs-driver-http`. Tokens are `cfs_secrets::Secret` behind the auth base — never logged, never in a `GmailError`. A **LEAF runtime consumer** — bridges its synchronous `PlanApplier` to the async `ApplyDriver` via `cfs-runtime`'s `PlanApplierBridge`; tokio dead-ends here. Parked for t20: attachment-bytes on-demand fetch, `historyId`/`@version` sync (E7), and the live smoke test (§5/§6/§10). |
| `driver-cf` | `cfs-driver-cf` | The **Cloudflare driver** (t23, RFD §5): one driver mounted at **`/cf`** fanning out to three Cloudflare primitives, each on its correct archetype. **D1** (`/cf/d1/<db>/<table>`, RelationalTable) is SQLite-over-HTTP — it **reuses the `cfs-sql-core` `Dialect::Sqlite` emitter + compiler** (the same injection-safe parameterized SQL t17 renders) and ships the rendered `(sql, params)` to the Cloudflare D1 REST API with `params` as a **structured bound array** (never interpolated); the D1 `/batch` endpoint maps one `commit_transaction` to **one atomic transaction** (D1 has no interactive BEGIN/COMMIT). **KV** (`/cf/kv/<ns>/<key>`, BlobNamespace): `ls/cp/mv/rm` + a degenerate `(key,value)` table for `SELECT`/`UPSERT`, TTL + metadata. **Queues** (`/cf/queue/<name>`, AppendLog): `INSERT` appends with an idempotency key (at-least-once-safe), `SELECT … LIMIT n` tails. Per-node capabilities gate verbs at parse time (e.g. `UPDATE`/`JOIN` over a queue/KV rejected structurally). The Cloudflare API token is a `cfs_secrets::Secret` written into a redacted `Authorization: Bearer` header — never logged, never in a `CfError`. **No vendor leak** — Cloudflare JSON + `worker::*` bindings decode to owned DTOs at the mockable `CfBackend` seam; the HTTP path rides a **local `HttpExchange` seam** over `cfs-http-core` DTOs (the `cfs-google-auth` precedent), so this crate does NOT depend on `cfs-driver-http` and stays an **independent runtime leaf** (it reuses the pure `cfs-sql-core`, not the `cfs-driver-sql` runtime leaf). Bridges its synchronous `PlanApplier` to the async `ApplyDriver`; tokio dead-ends here. Parked: the wasm `WorkersBindingBackend` (same seam) + live Cloudflare integration (§5/§6/§10). |

## Dependency spine (acyclic, no back-edges)

```
cfs (bin) → cfs-cmd → cfs-core → { cfs-lang, cfs-plan, cfs-driver, cfs-codec, cfs-types }
                         ▲
                   cfs-server ── depends on cfs-core
cfs-codec  → cfs-driver        (shared CfsError, decision D1 — acyclic)
cfs-codec  → cfs-types         (canonical Value/Row/RowBatch row model, t05 — acyclic)
cfs-driver → cfs-plan          (Driver::applier returns the PlanApplier seam; Plan nodes)
cfs-driver → cfs-types         (Driver::describe returns the canonical typed Schema, t13)
cfs-plan   → cfs-types         (effect nodes carry the canonical RowBatch/DriverId, t09 — acyclic)
cfs-txn    → { cfs-plan, cfs-types }  (t11 transactional envelope: PURE orchestration — EffectKey
                                idempotency, @version/ETag preconditions, CommitStrategy, saga/ACID
                                executors, audit ledger, RecoveryReport; NO tokio of its own)
cfs-runtime → { cfs-plan, cfs-types, cfs-txn }  (t10/t11 — tokio CONFINED here; the runtime bridges
                                its async ApplyDriver to cfs-txn's synchronous LegApplier seam)
cfs-parser → cfs-lang          (consumes the frozen keyword consts / AST)
cfs-pushdown → { cfs-driver, cfs-types, cfs-plan, cfs-parser }  (t14 pushdown planner: PURE.
                                PushdownProfile accessors from cfs-driver; the typed Predicate
                                + Schema::join from cfs-types; AST lowering from cfs-parser. No
                                I/O/async/vendor — acyclic, below cfs-core.)
cfs-engine → { cfs-pushdown, cfs-types }  (t14 local combine engine: the MiniEvaluator over the
                                PhysicalPlan. Dependency closure = serde family only; wasm-clean.)
cfs-core   → cfs-pushdown       (t14 integration seam: cfs_core::plan wires query AST → LogicalPlan
                                → PhysicalPlan via the live MountRegistry; surfaces ScanNodes for T10.)
cfs-secrets → cfs-types        (t27 credential/secret store + multi-account resolution: consumer-side,
                                owned-DTO, reuses the canonical DriverId. LEAF over cfs-types — acyclic.)
cfs-core   → cfs-secrets       (t27 bind-context credential surface: the Engine threads a Secrets
                                handle into the driver-bind context; cfs-core re-exports it.)
cfs-http-core → cfs-secrets    (t19 refinement: the shared PURE-LEAF HTTP DTO + redaction crate.
                                Owns HttpMethod/HttpRequest/HttpResponse + SENSITIVE_HEADERS/
                                is_sensitive_header + the redacting Debug — the SINGLE source of
                                truth for both HTTP seams. Among workspace crates it depends on
                                cfs-secrets ONLY (the REDACTED marker; reaches cfs-types only), and
                                carries NO reqwest/tokio/cfs-runtime — so depending on it does not
                                pull either HTTP crate toward the runtime. Acyclic leaf.)
cfs-driver-http → cfs-http-core (t19 refinement: the REST driver's request/response DTOs +
                                redaction now come from the shared leaf, not a local copy; its
                                HttpClient trait + reqwest impl stay local.)
cfs-google-auth → { cfs-secrets, cfs-http-core }  (t19 shared Google OAuth + multi-account auth
                                base. Among workspace crates it depends on cfs-secrets + cfs-http-core
                                ONLY (both reach no further than cfs-types), so it stays OFF
                                cfs-runtime: network rides a local, runtime-free HttpExchange seam
                                (over the shared cfs-http-core DTOs) rather than cfs-driver-http
                                (which depends on cfs-runtime and must remain a leaf). The HTTP DTOs
                                + redaction are single-sourced in cfs-http-core, not hand-copied.
                                Acyclic; not on the spine — consumed only by the future Google
                                driver leaves (t20/t21/t41).)
cfs-driver-local → { cfs-driver, cfs-plan, cfs-types, cfs-codec, cfs-runtime }  (t16 FIRST concrete
                                driver; LEAF runtime consumer — bridges the synchronous PlanApplier →
                                async ApplyDriver and registers it in the DriverRegistry. Nothing
                                depends back onto it, so tokio dead-ends here and never re-enters the
                                spine — the precedent the next 11 drivers follow.)
cfs-driver-gmail → { cfs-driver, cfs-plan, cfs-types, cfs-codec, cfs-runtime, cfs-google-auth,
                                cfs-http-core }  (t20 FIRST real Driver, mount /mail; LEAF runtime
                                consumer following the cfs-driver-local precedent — bridges its
                                synchronous PlanApplier → async ApplyDriver via cfs-runtime, so tokio
                                dead-ends here and never re-enters the spine. Rides the t19 auth base
                                (GoogleApiClient over the local HttpExchange); NO reqwest — that stays
                                in cfs-driver-http. Nothing depends back onto it.)
cfs-sql-core → cfs-types       (t23 extraction: the PURE-LEAF SQL compile/emit core — the
                                Dialect::Sqlite emitter + pure compiler + catalog DTOs + SqlError,
                                single-sourced so BOTH cfs-driver-sql and cfs-driver-cf reuse one
                                emitter. Among workspace crates it depends on cfs-types ONLY; carries
                                NO runtime/secrets/driver coupling. Acyclic leaf.)
cfs-driver-sql → { cfs-driver, cfs-plan, cfs-types, cfs-runtime, cfs-secrets, cfs-sql-core }  (t17
                                SQL driver; LEAF runtime consumer. Now reuses the pure cfs-sql-core
                                emitter rather than owning it; the runtime/secrets From adapters for
                                SqlError stay here.)
cfs-driver-cf → { cfs-driver, cfs-plan, cfs-types, cfs-runtime, cfs-sql-core, cfs-http-core,
                                cfs-secrets }  (t23 Cloudflare driver, mount /cf; LEAF runtime
                                consumer. Reuses the PURE cfs-sql-core (NOT cfs-driver-sql, a runtime
                                leaf) for the D1 sqlite emitter, and rides a LOCAL HttpExchange seam
                                over cfs-http-core (NOT cfs-driver-http) — so it stays an INDEPENDENT
                                runtime leaf and neither runtime leaf depends on the other. Nothing
                                depends back onto it.)
cfs-types  → (serde only)      (LEAF: no workspace deps; the vendor-free type model, t05)
```

Arrows point toward more-foundational crates. There are **no cycles** and **no
back-edges**. Mechanically enforced by `crates/cmd/tests/dep_direction.rs` (a
`cargo metadata` test).

### tokio confinement — the generic runtime-leaf rule (t10/t16)

`cfs-runtime` is the sole impure stage; tokio/futures live there and MUST NOT enter the
pure spine's closure (the invariant that keeps `cfs-plan`'s purity dep-closure test green).
`dep_direction.rs::runtime_is_confined_to_plan_and_types` enforces this in three parts:

1. **(a) source pinned** — `cfs-runtime`'s own workspace deps are exactly `{cfs-plan,
   cfs-types, cfs-txn}` (and `cfs-txn` is itself pure, carrying no tokio), so tokio's source
   is one crate.
2. **(b) generic leaf confinement** — **every** crate that depends on `cfs-runtime` must be a
   **leaf**: no workspace crate may depend back onto it. This encodes *why* a runtime consumer
   is safe (tokio dead-ends in a leaf and cannot transit back into the spine) rather than
   *which* crates were waved through. A non-leaf gaining the edge (e.g. `cfs-core → cfs-runtime`)
   fails automatically — **no per-driver test edit is needed** as the next 11 driver crates land.
3. **(b') identity allowlist** — a small named allowlist (`cfs-driver-local`, `cfs`) pins the
   *intent*: an unintended new runtime consumer is caught even if it happens to be a leaf today.
   A new driver-impl leaf appends its name here (a one-line, reviewable signal); the generic
   leaf check guarantees the append was safe.

`cfs-driver-local` is the first such leaf consumer: it bridges its synchronous `PlanApplier`
to the async `ApplyDriver` and registers it in the `DriverRegistry`.

### Decision D1 — where `CfsError` / `Path` live

design-v1 nominally placed `CfsError` and `Path` in `cfs-core`, but the `Driver` and
`Codec` trait signatures return `Result<_, CfsError>` and take `&Path`, while the
spine requires `cfs-core → cfs-driver`. Putting them in `cfs-core` would force a
cycle. They therefore live in **`cfs-driver`** (the lowest crate the signatures
need); `cfs-codec` depends on `cfs-driver` for the shared error, and `cfs-core`
**re-exports** both so the workspace still sees one `cfs_core::CfsError` /
`cfs_core::Path`. This preserves "one error enum, shared workspace-wide" while keeping
the spine strictly acyclic.

### Decision D2 — the `cfs-types` leaf (t05)

The canonical type & schema model (`Value`/`Row`/`RowBatch`, `Schema`/`ColumnType`,
`TypeError`, schema algebra, typed predicates) lives in a dedicated **leaf** crate
`cfs-types` (pre-reserved by the Architect). E0 shipped placeholder `Value`/`Row`/
`RowBatch` in `cfs-codec`; t05 promotes them to the canonical typed model and
`cfs-codec` now **re-exports** them from `cfs-types`, so the `bytes ↔ rows` boundary
and the evaluator speak one row model. The crate depends on **no other workspace
crate** (only `serde`/`serde_json`, the latter solely as the carrier for the `Json`
value of irregular columns), keeping it the lowest node in the spine and the type
model vendor-free (RFD §9, G6). To preserve that leaf status, `DriverId` is defined
*inside* `cfs-types` (an owned newtype, never a vendor handle) rather than imported
from `cfs-driver`, and the `SchemaSource` trait takes a logical segment list
(`&[Name]`) instead of the driver `Path`; E4 adapts the driver `Path` at the boundary.
`cfs-core` depends on `cfs-types` and re-exports it so the workspace sees one
`cfs_core::Schema` / `Value` / `TypeError`. Mechanically enforced by
`dep_direction.rs::types_is_a_leaf_and_codec_depends_on_it`.

### Wired edge — `cfs-core → cfs-parser` (acceptance criterion C5, E1)

The edge is now **wired** (E1): name resolution (`cfs_core::resolve`, t06) and the pure
evaluator (`cfs_core::eval`, t07) both consume the parsed `cfs_parser::Statement` AST —
the resolver binds `CALL`/alias names and gates effect verbs, and the evaluator folds a
statement into a `cfs_plan::Plan` (effect-plan) + logical `PlanSource` relation. The edge
is one-directional so `cfs-parser` can never depend on `cfs-core` (cycle prevention).
`dep_direction.rs::core_depends_on_parser_one_directionally` asserts the edge is present
and the back-edge absent.

## Boundary rules (must hold for every later ticket)

1. **Closed core / one keyword home (G1).** The reserved-keyword set lives only in
   `cfs-lang::KEYWORDS`. A new backend adds *zero* keywords — it registers a path,
   procedure, or codec instead. The freeze test (`cfs-lang`) locks the set.
2. **Open registries generic over trait objects (G2).** Extension = `register(...)`
   into one of the three `cfs-core` registries. Registries hold `Arc<dyn Driver>` /
   `Arc<dyn Codec>` / owned `ProcedureDecl`, never concrete types. `MountRegistry`
   additionally routes a full path to a driver by **longest mount-prefix**
   (`MountRegistry::resolve_path`), so overlapping mounts (`/g`, `/git`) resolve
   deterministically and the matched mount is stripped to a driver-local sub-path.
3. **Purity invariant at the type level (G3).** `Driver` / `Codec` methods return
   data (or `Plan` nodes); none take `&mut self`, return a future, or perform I/O. The
   only impure op (`COMMIT : Plan -> World`) is reserved for E2 and absent from the
   traits. Proven by the no-I/O dummy-impl tests in `cfs-driver` / `cfs-codec`.
4. **Server is a driver (G4).** `cfs-server` reserves `/server` as a mount and a
   `TODO(E7)` anchor; the server must be registered as a `Driver`, never a bespoke
   subsystem.
5. **cmd is logic-free (G5 / C4).** `cfs-cmd` depends on `cfs-core` + `cfs-server`
   only. Enforced by `dep_direction.rs`.
6. **Parser boundary reversibility (G6).** The chosen parser library's types never
   appear in `cfs-parser`'s public API — wrapped behind an owned `ParseError` and the
   `parse_statement` signature (t02).
7. **No vendor leak / owned DTOs (B3).** No SDK/vendor type crosses the `cfs-driver`
   boundary. DTOs are owned and `#[non_exhaustive]` with `new` constructors.
8. **No credentials (B8).** No secret/credential field anywhere; none in tests or CI.

## wasm-friendliness (B7)

The core crates (`cfs-core`, `cfs-lang`, `cfs-plan`, `cfs-driver`, `cfs-codec`,
`cfs-parser`) avoid threads, `std::fs`, and sockets so the future `wasm32` target
(RFD §1/§9) stays cheap. All real I/O lives behind (future) driver impls. `unsafe`
code is `forbid`-den workspace-wide.

## Cross-compile status

- **native `aarch64-unknown-linux-gnu`**: built & tested locally (this host is
  Graviton/aarch64). This is the local proof.
- **`x86_64-unknown-linux-gnu`**: lib crates cross-compile; the full binary link is
  **CI-only** (no x86_64 cross-linker on the local aarch64 host).
- **`wasm32-unknown-unknown`**: **deferred** per t01 — not built locally or in CI yet
  (a parked t02 / future-E0-sibling concern). Code is kept wasm-friendly per B7.

## Lints

Workspace lints (`Cargo.toml` `[workspace.lints]`): `unsafe_code = forbid`; clippy
`all = deny` plus `unwrap_used` / `expect_used` / `panic = deny` on non-test lib code.
Test modules opt out via `#![cfg_attr(test, allow(...))]` (and integration tests via a
file-level `#![allow(...)]`). Gates: `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo build --workspace`,
`cargo test --workspace`.
