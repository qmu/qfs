---
created_at: 2026-07-08T00:48:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure, DB, Config]
effort:
commit_hash: 32d9107
category: Changed
depends_on: [20260708004700-provisioning-reconcile-design-brief.md]
---

# Implement `qfs plan` / `qfs apply` — the provisioning reconcile loop

## Night-run status (2026-07-08) — mostly built; ONE leg open on an owner design decision

Built and committed across four green increments (`bbc38a3`, `7b4f901`, `b5b001d`, `f538c9a`):

- **`crates/provision/` (new `qfs-provision`)** — the pure core: canonical `.qfs` emitter (config
  projection only; runtime fields `last_run`/`cache_json` never emit, never drift), document loader
  (reuses the boot lower/apply seam), set-difference **diff** (absent⇒Remove, drift⇒Update, new⇒Insert)
  with a `ReconcilePlan` (add/change/destroy counts, `has_destroy`), and `build_plan` (destroys marked
  irreversible for `preview()`/the guard). Equality on `StatementSpec.canonical()` — cosmetic ≠ drift.
- **Both stores in the diff universe** — `/server` `ServerState` + the `/sys` collections `dump` covers.
  Policies keyed **per store** (never conflated). Secretish `sys_settings` **excluded, not redacted**;
  billing + `sys_ddl_events` structurally outside the universe (never destroyed by absence). The
  shipped **`restore` `<redacted>`-clobber flaw is fixed** (skips secretish/redacted values on replay).
- **Dispatching applier** — `ReconcileApplier` (generic over `PlanApplier`, DB-free) routes `/server`
  vs `/sys` nodes; sys backend seams completed (`sys` policy update/remove, setting/driver remove,
  audited + ddl_event in one tx). Dep-direction leaf-confinement restored (provision does **not**
  depend on `qfs-driver-sys`; the binary injects the concrete applier).
- **CLI `qfs plan` / `qfs apply`** — plan renders add/change/destroy (exit **0** no-changes / **2**
  changes-pending / **1** error, Terraform-style); apply commits, refusing distinctly on stale base
  (`--allow-stale-base`), destroy (`--commit-irreversible`), and host-not-serving. Generation-stamp
  parse+compare. `reemit_boot_config` (atomic temp-then-rename) built + unit-tested.
- **Docs/skills/versions** — cookbook `automation.md` recipe + regenerated `qfs-automation` skill;
  four plugin version fields **0.3.2 → 0.4.0**; qfs patch **0.0.28 → 0.0.29**. gen-docs/gen-skills/
  check-migrations all in sync.

**`/sys` store: fully END-TO-END** (fetch → plan → apply → converge → idempotent), hermetically tested.

**THE ONE OPEN LEG — design now SETTLED (§16 "The face, named", Decision X further amended
2026-07-08); wiring is increment 5.** The increment-4 investigation established the daemon already
has the face: the statement bridge `POST /api/describe|run|commit` (t52 — gated through the same
single `McpEngine` commit path as MCP, default-deny policy + irreversible gate). The reconcile CLI
becomes its third client; no new endpoint, no private RPC. Two wiring legs remain, both completions
of §10 server-is-a-driver:

1. **Read leg** — mount the introspective `/server` driver (read facet over the live `ServerState`
   snapshot) into the **serve composition's** engine + read registry only. The CLI's offline engine
   never mounts it (keeps `HostNotServing` honest).
2. **Write leg** — route `EffectKind::ServerConfigWrite` in the daemon's one commit path into the
   **live** `ServerConfigApplier` (the boot-replay lock; today every injected committer clones a
   throwaway registry — that routing is the whole fix), then `reconcile_all()`, the audit entry, and
   `reemit_boot_config` after a committed `/server` batch.
3. **CLI transport** — `fetch_current`'s `/server` half reads each collection through the bridge
   (§14 result envelope); `apply` submits the batch's CREATE≡INSERT twin statements
   statement-by-statement in plan order through `/api/commit` (the boot-replay shape; per-statement
   partial failure, re-plan converges). Retire `ServerFaceNotWired`.
4. **AuthZ hardening (the one new rule)** — a non-loopback bind without booted bearer material
   refuses the commit bridge fail-closed; otherwise the `/mcp` posture applies unchanged (bearer
   when the OAuth AS is booted, documented loopback-trust dev posture otherwise). `/server` writes
   need an explicit policy grant on the `server` driver; destroys carry the irreversible ack.

**Increment 5 LANDED** (commit `188195d`): all four legs wired — the `/server` read facet mounted
in the serve composition only, `ServerConfigWrite` routed into the live applier + runtime
audit/reconcile + boot re-emission, the CLI transport through `POST /api/run|commit` with
`ServerFaceNotWired` retired, and the non-loopback fail-closed rule. The bridge commit gate
resolves the live `/server/policies` row named `api` (codified in §16). Both stores are now
END-TO-END, hermetically proven (`server_reconcile_end_to_end_through_the_statement_bridge`,
`mixed_server_and_sys_document_reconciles_both_stores`, per-statement policy refusal, fail-closed
lock, offline-engine non-mount).

## Live verification evidence (2026-07-08, the Quality-Gate manual gate — PASSED)

Real `qfs serve` (debug binary, isolated `XDG_CONFIG_HOME`, `QFS_HTTP_ADDR=127.0.0.1:18791`,
boot config: `CREATE POLICY api ALLOW SELECT,INSERT,UPSERT,UPDATE,REMOVE,CALL ON server` + job
`nightly EVERY '1h'` + webhook `ingest`). Desired document: job → `'2h'`, + trigger `onmail`,
webhook omitted.

1. `qfs plan desired.qfs` → `Plan: 1 to add, 1 to change, 1 to destroy.`, destroy flagged
   irreversible, **exit 2** (changes pending). ✓
2. `qfs apply desired.qfs` (no ack) → refused: `the plan contains 1 destroy(s); re-run with
   --commit-irreversible`, **exit 1**, nothing applied. ✓
3. `qfs apply desired.qfs --commit-irreversible` → `apply committed: 3 effect(s) — 1 added,
   1 changed, 1 destroyed.`, **exit 0**. ✓
4. Re-`plan` → `No changes. The live configuration matches the document.`, **exit 0**
   (idempotent). ✓
5. Boot config **re-emitted** at its path (generation header + canonical twins + `CREATE POLICY`);
   contains `'2h'` and `onmail`, webhook gone. ✓
6. Daemon **restarted from the re-emitted config** → re-`plan` still `No changes` (the boot file
   is the at-rest form of the state it replays). ✓

All Quality-Gate items are now met (automated gates verified per increment; the manual live gate
above). This ticket is **complete**.

## Overview

Implement the Terraform-like declarative provisioning designed in the preceding brief
(`depends_on: 20260708004700-provisioning-reconcile-design-brief`). Promote the shipped `qfs dump`
(fetch) and `qfs restore` (additive apply) into a true **fetch → diff → reconcile** loop. Do **not**
re-decide any settled axis; build them:

- **Unified fetch:** read the **whole** current config across both stores — the `/server`
  `ServerState` collections (endpoints/triggers/jobs/views/policies/webhooks) unioned with the `/sys` /
  project system-DB current state `dump` already covers (connections, policies, settings, path
  bindings) — and emit it as a **canonical `.qfs` "as code" script** (a normalized list of `CREATE …`
  statements). Secret-by-reference only (no vault ciphertext/token). Stamp a config generation
  (migration counts + `ddl_event` chain head). The emitted document and its equality cover the
  **config projection only**: runtime fields (`ViewDef.last_run`/`cache_json`, `JobDef.last_run`) are
  never emitted, never drift, and are **preserved** by an `Update`. **Secretish `sys_settings` are
  excluded, not redacted** (blueprint §16, amended): never emitted, never diffed, never destroyed by
  absence — and fix the shipped `restore` flaw in the same PR (it currently writes the dump's literal
  `<redacted>` back over live secretish settings; it must skip them, counted).
- **The process boundary (blueprint §16, amended — the transport ruling):** `qfs dump`(canonical)/
  `plan`/`apply` address a **host** (§8; local = the local daemon at its loopback bind). The system-DB
  half is read/written directly as today; the `/server` half is fetched from and applied **through the
  running daemon's public statement face** (read `/server/*`, submit the batch's `ServerConfigWrite`
  statements — no privileged config API), so the commit and `reconcile_all()` run **inside the daemon
  process** and live causes converge. No running daemon ⇒ the `/server` half is a structured
  **host-not-serving refusal**, never an empty current state. After every committed `/server` batch
  the daemon **re-emits its post-commit `ServerState` as the canonical document at its boot config
  path** (atomic temp-then-rename via the durable-store seam) so an applied reconcile survives
  restart.
- **Diff / plan (`qfs plan`):** compute desired-vs-current **set difference** per collection into
  `ServerWriteOp::{Insert, Update, Remove}` + system-DB deltas; equality decided by
  `StatementSpec.canonical()` so drift is exact and cosmetic formatting is **not** drift. Policies are
  keyed by name **within their store** (`/server/policies` vs `sys_policies` — two collections, never
  conflated). Excluded collections (billing, `sys_ddl_events`, secretish settings) are outside the
  diff universe entirely — authoritative destroy can never touch them. Render via
  `preview()` as an **add / change / destroy** summary with the irreversible-node list; `plan`'s exit
  code distinguishes "no changes" from "changes pending". `plan` is pure — it writes nothing.
- **Apply / reconcile (`qfs apply`):** build one batch `Plan`, drive `commit()` through a thin
  **dispatching applier** (`commit()` takes one `PlanApplier`; the dispatcher routes `/server` nodes
  to `ServerConfigApplier` and `/sys` nodes to the `SystemDbBackend` path — a router, not a third
  applier), then `reconcile_all()` so live causes (HTTP router, watchtower) converge. Emit
  `sys_ddl_events` for every applied effect.
- **Authoritative desired-state:** absent-in-desired ⇒ **REMOVE**, drifted ⇒ **UPDATE**, new ⇒ **INSERT**.
  Re-applying an unchanged desired state ⇒ **empty plan / no-op** (idempotent).
- **Safety:** any **destroy** in the plan makes `apply` require `--commit-irreversible`; without the ack
  it is refused (never conflated with a policy denial). CREATE POLICY gate applies independently.
  **Stale base:** `apply` **refuses** when the document's generation stamp mismatches the live chain
  head unless `--allow-stale-base` is passed (blueprint §16, amended — three independent controls:
  stamp consent, irreversible ack, policy; any one refuses alone). `plan` renders the base-moved flag.
- **Surface:** new top-level verbs `qfs plan` / `qfs apply` on the existing dump/restore + `qfs-plan`
  machinery — **no new frozen keyword** (keyword count stays 39). SemVer = MINOR.
- **Docs & versioning:** regenerate `docs/{language,drivers,server}.md` (`gen-docs`), add a cookbook
  recipe parse-checked by `crates/test/tests/cookbook_skills.rs`, regenerate skills, bump the four plugin
  version fields (taught surface → minor) and the qfs patch. Any config-schema change ships a **new**
  migration.

## Key files

- `crates/server/src/state.rs` (`ServerState`, `snapshot()`) + `crates/server/src/runtime.rs`
  (`apply_source`, `reconcile_all`) — the desired-state document + apply/reconcile path.
- `crates/server/src/driver.rs` (`apply_server_write` → `ConfigChange`) + `crates/server/src/binding.rs`
  (`Binding::reconcile`) — the per-row convergence + live-cause reconcile seam.
- `crates/qfs/src/dump.rs` — extend the fetch to also emit `/server` `ServerState` and produce the
  canonical `.qfs` script form (today it is system-DB JSONL only).
- `crates/qfs/src/restore.rs` — the preview/commit + idempotency contract to generalize from insert-or-
  skip into update/remove reconcile (see `restore_previews_then_commits_a_dump_idempotently`); **also
  fix here**: skip (never write) `<redacted>` secretish setting values on replay.
- `crates/plan/src/server.rs` (`ServerNode`, `ServerWriteOp::{Insert,Update,Remove,Upsert}`),
  `crates/plan/src/preview.rs` (`preview`, `Preview`), `crates/plan/src/apply.rs`
  (`commit`, `PlanApplier`, `CommitReport`) — the diff verbs + the batch plan/commit engine. `commit()`
  takes **one** `PlanApplier` and `ServerConfigApplier` refuses foreign nodes (`runtime.rs`), so the
  mixed `/server`+`/sys` batch needs the **new dispatching applier** (small, in the binary/serve
  composition).
- `crates/core/src/ddl/server.rs` (CREATE≡INSERT desugar, `from_server_ddl`) +
  `crates/core/src/ddl/server/spec.rs` (`StatementSpec.canonical()` — drift equality) +
  `crates/core/src/ddl/connections.rs` (connections `.qfs`, secret-by-reference).
- `crates/store/src/ddl_events.rs` (`ChainedDdlEvent`, `verify_chain`) — audit spine for applied effects.
- `crates/store/src/migrate.rs` + `crates/qfs/src/migration_guard.rs` — migration immutability; new
  migration if the config schema changes.
- `crates/cmd/src/lib.rs` (`Command::{Dump,Restore}`, action structs, launcher seams) — add
  `Command::{Plan,Apply}` mirroring the preview-by-default / `--commit` convention.
- `crates/qfs/src/serve.rs` (`run_serve` boot) — the live daemon the reconcile hot-reconfigures.
- `crates/qfs/src/docs.rs` (`render_server`) + `xtask` gen-docs/check-migrations gates.
- Version fields: `plugins/qfs/.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, both `version`
  in `.claude-plugin/marketplace.json`; qfs patch in `crates/qfs/Cargo.toml`.

## Related history

- Design brief `20260708004700-provisioning-reconcile-design-brief` (parent) — settled semantics.
- `20260707022411-dump-current-qfs-state` / `20260707022412-restore-and-replay-qfs-state` — the fetch/
  apply precursors extended here.
- t30 server-runtime-and-self-config-driver; t42 persistence-sqlite-system-project-db; t11 transactions-
  idempotency-concurrency — the substrate.

## Implementation steps

1. **Unified fetch (through the host):** extend `dump` to union the `/server` `ServerState` collections
   — fetched via the running daemon's public statement face (§16's transport ruling; host-not-serving
   is a structured refusal) — with the system-DB current state, and emit a canonical `.qfs` script
   (secret-by-reference; config projection only — no runtime fields, no secretish settings, no
   billing/events); stamp the config generation.
2. **Diff engine:** desired-vs-current set difference per collection → `ServerWriteOp::{Insert,Update,
   Remove}` + system-DB deltas; equality via `canonical()` on the config projection; policies keyed per
   store; excluded collections outside the universe; unchanged ⇒ empty.
3. **`qfs plan`:** render the diff through `preview()` as an add/change/destroy summary with the
   irreversible list + the base-moved flag; exit code distinguishes empty/non-empty; writes nothing.
4. **`qfs apply`:** build the batch `Plan`, `commit()` via the **dispatching applier**
   (`ServerConfigApplier` for `/server` — submitted through the daemon's face so `reconcile_all()`
   runs in-process — + `SystemDbBackend` for `/sys`), emit `ddl_events`; destroy ⇒ require
   `--commit-irreversible`; stale stamp ⇒ refuse without `--allow-stale-base`; an `Update` preserves
   runtime fields; the daemon re-emits its boot config post-commit (durability).
5. **CLI:** add `Command::{Plan,Apply}` + action structs + launcher seams, preview-by-default.
6. **Fix `restore`:** skip `<redacted>` secretish setting values on replay (the shipped round-trip
   clobbers live values today), counted in the report.
7. **Docs/skills/version:** `gen-docs`, add cookbook recipe, `gen-skills`, bump four plugin versions +
   qfs patch; new migration if the schema changed.
8. **Tests:** hermetic across the spine (no network/credentials) — add/change/destroy reconcile, round-
   trip idempotency, drift equality, destroy-gate ack present/absent, stale-base refusal/override,
   secretish-settings exclusion, runtime-field preservation, excluded-collections-never-destroyed,
   secret-by-reference assertion.
9. **Live verification:** one manual `qfs serve` → fetch → edit → `qfs plan` → `qfs apply` round exercised
   against a real running daemon, evidence recorded on the ticket.
10. **Transform SoT cross-check:** if the transform implementation (ticket
    `20260708002200-transform-predicate-implementation`) has already shipped, this ticket owns adding
    `/transform` definitions to the SoT emitter + differ (blueprint §15/§16); otherwise record the
    obligation on that ticket.

## Considerations

- Reuse dump/restore + `qfs-plan`; this is a promotion, not a new subsystem.
- Experimental / no backward compat: no migration/deprecation subsystem; destructive convergence rides
  the existing irreversible gate.
- Commit via `workaholic:commit` `commit.sh` with explicit file args (shared-tree); never `git add -A`
  (also guards against staging untracked credential files). Do not pipe `cargo fmt --check` through
  tail/head. clippy is `--workspace --all-targets -D warnings`, not `--all-features`.

## Quality Gate

**Verification method** (objective, checkable before `/drive` approval):

- [ ] `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -D
      warnings`, `cargo fmt --all --check` all pass; the whole suite stays **hermetic** (no network,
      no credentials).
- [ ] Unit/e2e tests cover the **three reconcile cases**: an absent-in-desired row is **REMOVED**, a
      drifted row is **UPDATED**, a new row is **INSERTED** — asserted on the resulting `ServerState` /
      system-DB and on the plan's add/change/destroy counts.
- [ ] **Round-trip idempotency:** fetch → `apply` → re-fetch yields the same canonical document, and a
      second `apply` of the unchanged desired state produces an **empty plan** (no-op) — asserted.
- [ ] **Drift equality:** a desired document differing from current only in cosmetic source formatting
      plans to **zero** changes (canonical-spec compare), asserted.
- [ ] **Destroy gate:** a plan containing a destroy is **refused** by `apply` without
      `--commit-irreversible` and **applied** with it — both directions tested; a policy denial is a
      distinct error from a missing ack.
- [ ] **Secret safety:** the fetched `.qfs` SoT contains only `env:`/`vault:` **references** — no vault
      ciphertext or token appears — asserted by a test scanning the emitted document. **Secretish
      settings are absent entirely** (not redacted): never emitted, never planned as drift, never
      destroyed by absence; and `restore` now **skips** `<redacted>` secretish values instead of
      writing the literal back — both asserted.
- [ ] **Config projection:** runtime fields (`ViewDef.last_run`/`cache_json`, `JobDef.last_run`) are
      not emitted, do not read as drift, and **survive an `Update`** — asserted (a refresh between
      fetch and apply plans zero changes and keeps its cache).
- [ ] **Excluded collections:** billing rows and `sys_ddl_events` absent from the desired document are
      **never** planned for destroy — asserted.
- [ ] **Stale base:** `apply` with a mismatched generation stamp is **refused** without
      `--allow-stale-base` and proceeds with it; `plan` renders the base-moved flag — all asserted;
      the refusal is a distinct error from a missing irreversible ack and from a policy denial.
- [ ] **Process boundary:** the `/server` half is fetched/applied through the daemon's public statement
      face; with no daemon running, `plan`/`apply` render a structured **host-not-serving** refusal for
      the `/server` half (never an empty current state / whole-document adds) — asserted. After a
      committed `/server` batch the daemon re-emits its boot config (restart converges) — asserted in
      the live check.
- [ ] `keyword_count_is_frozen` still asserts **39** (no new frozen keyword); `qfs plan`/`qfs apply` are
      CLI verbs, preview-by-default.
- [ ] `cargo run -p xtask -- gen-docs --check`, `gen-skills --check`, `check-migrations` all pass;
      `docs/{language,drivers,server}.md` regenerated; one cookbook recipe using `qfs plan`/`qfs apply`
      is present and parse-checked green.
- [ ] Version bumps applied in the **same PR**: four plugin `version` fields minor-bumped +
      `crates/qfs/Cargo.toml` patch-bumped.
- [ ] **Live reconcile verification:** one manual run against a real `qfs serve` daemon — fetch the
      config, edit it (add one binding, change one, remove one), `qfs plan` (observe add/change/destroy
      counts), `qfs apply --commit-irreversible`, and confirm the live bindings (HTTP route / watchtower)
      **converged**. Evidence (commands + observed plan + post-apply state) recorded on the ticket. This
      is the one non-hermetic check, run once out of band.

**Acceptance criteria:** `qfs plan` shows an honest add/change/destroy diff of desired-vs-current across
both stores, `qfs apply` converges the live daemon to the desired state (creating, updating, and removing
as needed), the loop is idempotent (unchanged ⇒ no-op), destroy is gated by the irreversible ack, the SoT
is secret-free, the keyword set stays frozen at 39, all docs/skills/versions are updated, and one live
round has been observed.

**The gate that must pass:** every checkbox above — the automated half is the green build + all
reconcile/idempotency/drift/destroy-gate/secret tests + the four `--check` ratchets + the frozen-keyword
assertion; the manual half is the recorded live-daemon reconcile.

**Edge cases the tests must cover:** unchanged desired state ⇒ empty plan; cosmetic-only diff ⇒ zero
changes; removing a binding still bound to a live cause; a stale config generation (base moved since
fetch) refused without the override; a cross-store apply that partially fails (`CommitReport.applied`
lets it be re-run); secrets never entering the SoT; secretish settings and excluded collections never
destroyed by absence; a materialized-view refresh between fetch and apply is not drift; a policy name
present in both stores diffs independently per store; an unreachable daemon never reads as empty
`/server` state.

**Division of assurance:** design coherence is owned by the parent brief ticket; this ticket owns the
runtime gate — hermetic tests + ratchets (automated) and the single recorded live-daemon reconcile
(manual). The `.qfs` SoT is secret-by-reference, asserted by test.
