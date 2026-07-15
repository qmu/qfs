---
created_at: 2026-07-08T00:47:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash: ef67876
category: Added
depends_on: []
---

# Design brief: Terraform-like provisioning — `qfs plan` / `qfs apply` reconcile loop

## Overview

Write the **design brief** (a full prose brief, authored with **Fable**) for a declarative,
Terraform-style **provisioning / reconcile** mechanism over the qfs server, and land it as an in-place
extension of **`docs/blueprint.md` §10 (Server — server is a driver)** and **§13 (a driver is data)**
(revise in place, status-tag the new material `blueprint`; no new numbered ADR/RFD).

The thesis: qfs already makes server config **data** (`/server/*` bindings + `/sys/*` system-DB state),
and `DESCRIBE → write → PREVIEW → COMMIT` is *already* a fetch→desired→plan→apply reconcile. The
just-shipped `qfs dump` (fetch) and `qfs restore` (additive, preview-by-default apply) are the
precursors. This design **promotes them into a true reconcile loop**: an AI coding agent fetches the
**whole** current configuration as one editable **"server as code"** document, edits it, and applies it
back as the **authoritative desired state** — the interpreter computes the desired-vs-current **diff**
and converges (create / update / **remove**). This is experimental / pre-release qfs: make the
definitive call, no backward-compat or migration subsystem, no risk framing.

### Decisions already settled (the brief specifies, does not re-open them)

1. **Reconcile semantics = authoritative desired-state (add / change / destroy).** The fetched document
   is the single source of truth. Rows present in current but **absent** in desired are **REMOVED**;
   drifted rows are **UPDATED**; new rows are **INSERTED**. (Contrast today's `restore`, which is
   insert-or-skip and never removes/updates — that gap is exactly what this closes.) REMOVE is
   inherently irreversible and rides the existing irreversible gate.
2. **Source-of-truth artifact = a canonical `.qfs` "as code" script.** The whole config is fetched and
   emitted as a normalized list of `CREATE ENDPOINT|TRIGGER|JOB|VIEW|POLICY|WEBHOOK` + `CREATE
   CONNECTION` statements (plus the relevant `/sys` settings/path-bindings). The **CREATE ≡ INSERT**
   equivalence and `StatementSpec.canonical()` span-normalization make the round-trip exact and the diff
   byte-stable (cosmetic source differences must **not** read as drift). A JSONL state export may remain
   the machine/backup form, but the `.qfs` script is the authoritative, agent-editable SoT.
3. **Surface = a short top-level verb pair `qfs plan` / `qfs apply`.** `plan` = PREVIEW of the reconcile
   diff (pure, touches nothing); `apply` = COMMIT of the reconcile. Built on the existing dump/restore
   machinery and the pure `qfs-plan` effect substrate — **no new frozen keyword** (contextual verbs on
   the CLI), so it is **MINOR** under the SemVer policy. (Naming aligns with the operator's mental model
   and the "prefer short top-level commands" convention.)
4. **Scope = whole config spanning BOTH stores**, secret-by-reference only: `/server` `ServerState`
   (endpoints/triggers/jobs/views/policies/webhooks) **and** the `/sys` / project system-DB config that
   `dump` already covers (connections, policies, settings, path bindings). The document carries **no**
   vault ciphertext / tokens (secrets stay `env:`/`vault:` references); the audit chain and billing are
   out of the editable SoT.

### The shape the brief must specify

- **Unified fetch.** How the whole current config is read across both stores into one document —
  `Runtime::snapshot()` (`ServerState`) unioned with the `dump` system-DB current-state, emitted as the
  canonical `.qfs` script. A **generation / config-version** stamp (migration counts + `ddl_event` chain
  head) so plan/apply can detect a stale base.
- **The diff / plan.** desired-vs-current **set difference** per collection → `ServerWriteOp::{Insert,
  Update, Remove}` (server side) and the corresponding system-DB deltas. Define how equality is decided
  (canonical spec compare) so drift is exact. The plan is rendered by `preview()` as an **add / change /
  destroy** summary (terraform-style counts) with the irreversible-node list called out.
- **The apply / reconcile.** Build one batch `Plan`, drive `commit()` through the existing appliers
  (`ServerConfigApplier` / `SystemDbBackend`), then `Binding::reconcile()` converges live causes (HTTP
  routes, watchtower). Idempotency: re-applying an unchanged desired state yields an **empty plan**
  (no-op). Every applied effect is recorded in the hash-chained `sys_ddl_events` WORM tail.
- **Safety.** `plan` never writes; `apply` with any **destroy** requires the explicit
  `--commit-irreversible` ack (a policy denial is never conflated with a missing ack); the CREATE POLICY
  gate applies independently. Cross-store applies reckon with the blueprint's cross-source
  best-effort/partial-failure recovery (not silent atomicity across `/server/*` + `/sys/*`).
- **Versioning & anti-drift.** State the SemVer verdict (MINOR — new CLI verbs/registry surface, no
  grammar change), and that shipping regenerates `docs/{language,drivers,server}.md`, regenerates skills,
  and — since the surface is skill-taught — bumps the four plugin version fields; any config-schema
  change ships a **new** migration (never edits a shipped one).

## Key files (read while writing the brief)

- `docs/blueprint.md` §10 (line ~229, *implemented*) + §13 (line ~321, *blueprint*) — the sections this
  brief extends in place.
- `crates/server/src/state.rs` (`ServerState`) + `crates/server/src/runtime.rs` (`Runtime::boot` /
  `apply_source` / `reconcile_all`) — the desired-state document + the canonical apply path.
- `crates/qfs/src/dump.rs` (`qfs dump`, `qfs-state-jsonl-v1`, secret-free) + `crates/qfs/src/restore.rs`
  (`qfs restore`, preview/commit, insert-or-skip) — the precursors this generalizes; note the `restore`
  idempotency test as the round-trip contract to extend.
- `crates/plan/src/server.rs` (`ServerNode` / `ServerWriteOp::{Insert,Upsert,Update,Remove}`),
  `crates/plan/src/preview.rs` (`preview → Preview`), `crates/plan/src/apply.rs` (`commit`, `PlanApplier`,
  `CommitReport`) — the diff verbs + plan/apply substrate.
- `crates/core/src/ddl/server.rs` (CREATE≡INSERT desugar) + `crates/core/src/ddl/server/spec.rs`
  (`StatementSpec.canonical()` — byte-stable bodies for exact drift) + `crates/core/src/ddl/connections.rs`
  (declarative connections file, secret-by-reference).
- `crates/store/src/ddl_events.rs` (hash-chained provenance) + `crates/store/src/migrate.rs` /
  `crates/qfs/src/migration_guard.rs` (migration immutability).
- `crates/cmd/src/lib.rs` (`Command::{Dump,Restore}`, action structs, launcher seams — where
  `plan`/`apply` verbs slot in) + `crates/qfs/src/docs.rs` (`render_server`, gen-docs).

## Related history

- **`20260707022411-dump-current-qfs-state.md`** (`qfs dump`) and **`20260707022412-restore-and-replay-qfs-state.md`**
  (`qfs restore`) — the shipped fetch/apply precursors this design promotes into a reconcile loop.
- **t30 server-runtime-and-self-config-driver** — server-is-a-driver / self-config path (blueprint §10).
- **t42 persistence-sqlite-system-project-db** — the system-DB config store.
- **t11 transactions-idempotency-concurrency** — the idempotency/transaction substrate a batch apply rides.
- Blueprint §13 *a driver is data* — the declare/store/activate stance this generalizes to all config.

## Implementation steps (this ticket produces a design doc, not code)

1. Read the key files; confirm the current `dump`/`restore` behavior and the exact `ServerWriteOp` /
   preview / commit shapes.
2. Draft the brief (state / options / trade-offs / recommendation), pinning the four settled decisions
   and specifying unified fetch, the diff (equality via canonical specs), the add/change/destroy plan,
   the reconcile apply, and the destroy/irreversible + policy gates.
3. Land it as extended §10/§13 material in `docs/blueprint.md`, revised in place, with a decision id and
   `status: blueprint`.
4. Enumerate the implementation surface precisely enough that the implementation ticket (`depends_on`
   this one) needs no further design decisions.

## Considerations

- Author with **Fable** — genuine design judgment (reconcile semantics, drift equality, cross-store apply).
- Reuse, don't reinvent: this is dump/restore + the `qfs-plan` substrate promoted, not a new subsystem.
- No migration/deprecation subsystem — reconcile is set difference; destructive convergence rides the
  existing irreversible gate (blueprint §5 "redefinition, not migration").
- Keep the `.qfs` SoT **secret-by-reference** — the document must be commit-safe (no vault ciphertext).

## Quality Gate

**Verification method** (objective, checkable before `/drive` approval):

- [ ] `docs/blueprint.md` §10/§13 carry a new provisioning/reconcile section with a **decision id** and
      `status: blueprint`, revised **in place** (no new ADR file added).
- [ ] The section rules on **all four** settled axes explicitly: (1) authoritative desired-state
      add/change/**destroy** with REMOVE behind the irreversible gate; (2) canonical `.qfs` "as code"
      SoT with CREATE≡INSERT round-trip and canonical-spec drift equality; (3) `qfs plan` (PREVIEW diff)
      / `qfs apply` (COMMIT reconcile) verbs, no new frozen keyword, SemVer = MINOR stated; (4) whole-
      config scope across `/server` + `/sys`, secret-by-reference, audit/billing excluded.
- [ ] The unified-fetch, diff-equality, batch-apply/reconcile, idempotency (empty plan on unchanged
      state), and cross-store partial-failure semantics are specified concretely enough to implement
      without further decisions.
- [ ] `cargo run -p xtask -- gen-docs --check` and `cargo fmt --all --check` still pass (no generated-doc
      drift or formatting break from the edit).

**Acceptance criteria:** a reviewer can read the brief and know exactly what the implementation ticket
must build — the fetch document, the diff, the plan output (add/change/destroy), the reconcile apply, the
destroy/irreversible + policy gates, and the version impact — without asking a question.

**The gate that must pass:** the four checkboxes above (verified by grep + the two `--check` commands).
Design coherence with blueprint §5 (redefinition-not-migration), §7 (preview/commit + irreversibility),
§8 (policy) is the subjective half a human confirms at `/drive` approval.

**Edge cases the brief must address:** an unchanged desired state must plan to a **no-op**; a drifted row
whose only difference is cosmetic source formatting must **not** read as a change (canonical spec); a
desired state that removes a binding still referenced by a live cause; a stale base (config generation
moved since fetch); a cross-store apply that partially fails; secrets must never enter the `.qfs` SoT.

**Division of assurance:** this ticket owns the **design** gate only. Test hermeticity, the
add/change/destroy reconcile tests, round-trip idempotency, and the live-daemon reconcile check are owned
by the implementation ticket that `depends_on` this one.
