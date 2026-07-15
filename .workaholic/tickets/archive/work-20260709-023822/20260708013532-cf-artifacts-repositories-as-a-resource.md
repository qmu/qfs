---
created_at: 2026-07-08T01:35:32+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
commit_hash: 8ca0522
category: Added
depends_on:
mission:
---

# Support Cloudflare Artifacts: create/list/delete Git repositories as a `/cf/artifacts` resource

## ⏸ NIGHT-DRIVE 2026-07-09 — deferred to a full-context session (blocker + risk, not size)

This ticket was reached in the same night `/drive` that shipped the whole transform epic (review
fixes + T2→T4, four PRs' worth on branch `work-20260709-023822`). After mapping the full driver-cf
surface (`path.rs`/`schema.rs`/`registry.rs`/`effect.rs`/`applier.rs`/`backend.rs`/`lib.rs`) I
deliberately did **not** implement it in this session — two reasons, neither of which is size:

1. **Named hard external blocker on the required live gate.** The owner's Option-A decision
   unblocked the *design* tension, but the Quality Gate still REQUIRES a live
   `create → verify remote_url + token sealed + no leak → delete` round-trip, and states the
   ticket "is not approvable" on hermetic tests alone if the account lacks Artifacts access. The
   connected account's **private-beta Artifacts access is unverified**, and the
   `qfs-env-has-live-cloud-accounts` rule forbids probing live cloud without an explicit ask — so
   the gate is unreachable in an autonomous night run.
2. **The minted repo token is the flagged "crown jewel"** (it grants push). Its vault-sealing must
   be provably leak-free (the ticket's explicit no-leak assertions). Implementing security-critical
   token-handling rushed at the tail of a long multi-ticket context is exactly the regression risk
   the `own-your-regressions` discipline says to avoid; it deserves a fresh full-context session.

**Concrete implementation plan for the resuming session (all patterns verified this session):**
- `path.rs`: add `CfNode::Artifacts` (the `/cf/artifacts` list node, account-scoped) and
  `CfNode::ArtifactRepo { namespace, name }` (`/cf/artifacts/<ns>/<name>`), an `ARTIFACTS_SEGMENT`,
  parse arms, and an `account_selector` returning `None` (account-scoped, not per-resource keyed).
- `schema.rs`: `artifacts_repos_schema()` = `(namespace TEXT, name TEXT, id TEXT, remote_url TEXT,
  created_at TEXT)` — **no token column** (structurally cred-free, like `/transform`'s secret_ref
  omission).
- `registry.rs`: Artifacts is **account-scoped**, so add an optional account-level backend to
  `CfRegistry` (the per-resource `d1/kv/queues` maps don't fit) plus an injected **`TokenSealer`**
  seam (a trait `seal(&self, repo_key, token: Secret) -> Result<(), _>`) — mirror the crate's
  injected-backend pattern so hermetic tests use a `MockSealer` that records the seal and a no-leak
  test asserts the token never reaches any rendered row/RecordedCall/error.
- `backend.rs`: `CfBackend::{list_artifacts, create_artifact_repo (→ remote_url + minted Secret),
  delete_artifact_repo}` — the create leg is `POST repos` then `POST tokens` (mint is a separate
  call, per the pre-verified REST paths in the Overview) then hand the `Secret` to the sealer; add
  `MockCfBackend` seed methods + three `RecordedCall` variants. DTOs stay crate-private.
- `effect.rs` + `applier.rs`: `CfEffect::ArtifactCreate` (UPSERT — read-after-write, Option A: the
  write returns only affected; a follow-up `SELECT /cf/artifacts` shows `remote_url`) and
  `ArtifactDelete` (REMOVE, **irreversible** — mirror github.merge / per-MAP IRREVERSIBLE gate). The
  applier seals the minted token via the injected sealer and surfaces only the non-secret row.
- `lib.rs`: `caps_for` arm (`Artifacts` → SELECT/INSERT/UPSERT; `ArtifactRepo` → SELECT/REMOVE;
  reject JOIN/aggregate/UPDATE at the parse gate), a `describe` arm (RelationalTable), and an
  `artifacts_list` read method.
- `cf.rs` (binary): register the account backend on a connected `/cf` mount + wire the real
  vault-sealer adapter (reuse `secret_store`/`vault` — the `networked_credential` seal path); the
  live `remote_url`/token round-trip is the owner-approved step.
- `tests.rs`: hermetic `MockCfBackend` + `MockSealer` — create returns a row with `remote_url` and
  NO token; the token is asserted **absent** from stdout/JSON/RecordedCall/errors; list; REMOVE
  gated by the irreversible flag; verb-gating rejections; fail-closed no-access error.
- Docs: a `/cf/artifacts` cookbook recipe + regenerate `drivers.md`/skills; qfs patch bump + a
  **minor** plugin bump (whole new `/cf` resource, taught surface).
- **Live gate (owner-gated):** create → verify `remote_url` returned + token sealed + no leak →
  delete against a beta-enabled account (the `git clone` proof moves to the git-handoff follow-up
  per Blocker 2). Do NOT run autonomously.

## ✅ UNBLOCKED — owner decision recorded 2026-07-09 (Option A + re-scoped live gate)

The owner resolved both blockers on 2026-07-09 (during the `/drive` that started the Transform
epic): **adopt Option A (read-after-write)** for the create-return model, and **re-scope the live
gate** to `create → verify remote_url returned + token sealed + no leak → delete` (the `git clone`
proof moves to the git-handoff follow-up, per Blocker 2). This ticket is now implementable; it was
not built during that drive (which drove the Transform chain instead), so it stays in `todo/` as
ready work with the design tension resolved. When implemented:

- Create = `UPSERT` mints the repo + seals the repo-scoped token into the vault; the caller then
  `SELECT /cf/artifacts` (a GET) to read the new repo's `remote_url` — the literal "returned row
  carries `remote_url`" below softens to "a follow-up SELECT shows it," consistent with D1/KV, with
  **zero new engine machinery** (no RETURNING channel, no `CALL` surface).
- Live gate = `create → remote_url returned + token sealed + no leak → delete`; drop the `git clone`
  step (it needs the deferred vault→git token handoff) and move that proof to the git-handoff
  follow-up.

The original two-blocker analysis is retained below for context.

## ⚠ BLOCKER ANALYSIS (found 2026-07-08, pre-drive) — resolved above

A pre-implementation review (during the night `/drive` that shipped the sibling declared-`/cloudflare`
ticket) surfaced **two blockers**. The locked model does not fit the engine as written, and the
required live gate is unreachable in this ticket's scope. No code was written for this ticket; the
second Cloudflare ticket (`20260708023259`, declared `/cloudflare` driver) shipped instead.

### Blocker 1 — "a repo is a resource, create returns its `remote_url`" collides with the write path

The ticket LOCKS: *create = `INSERT`/`UPSERT` into `/cf/artifacts`; **the returned row carries the
`remote_url`***. But a qfs write cannot return a row:

- `qfs_runtime::EffectOutput` is `{ id, affected: u64 }` (`packages/qfs/crates/runtime/src/outcome.rs`)
  — a write surfaces only an affected **count**. There is **no RETURNING / rows-from-write** channel
  in any driver (D1/KV `UPSERT`, `CALL github.merge` — all return a count).
- The owner also forbade a `CALL` procedure for the lifecycle ("resist re-introducing a CALL
  surface"), which is the only existing shape that returns rows.

So the core "create returns `remote_url`" is not expressible today. Three ways forward:

- **Option A — read-after-write (recommended).** `UPSERT` creates the repo and seals the token; the
  caller then `SELECT /cf/artifacts` (a GET) to read the new repo's `remote_url`. Zero new engine
  machinery; consistent with D1/KV, which also do **not** echo the written row. The only cost is the
  ticket's literal "returned row carries `remote_url`" softens to "a follow-up SELECT shows it."
- **Option B — add a RETURNING channel** to `EffectOutput` so a write carries back rows. Cross-cutting
  engine change touching every driver + commit-output rendering. Large blast radius; only worth it if
  write-returns-row is wanted broadly (not just here).
- **Option C — deterministic `remote_url`.** If the Git remote URL is computable from
  `(account, namespace, name)` with no server round-trip, create needs no return value and
  `SELECT`/`DESCRIBE` synthesize it. Depends on the (unconfirmed) Artifacts URL scheme.

### Blocker 2 — the required live gate is unreachable in this ticket's scope

The Quality Gate below requires a live **create → `git clone` the `remote_url` → delete** round-trip.
But (a) the connected account's Artifacts (private-beta) access is **unverified** — the token
currently returns `api_status` on some `/cf` resources — and (b) the clone step needs the vault→git
**token handoff, which this ticket itself defers** as a follow-up ("does not implement the Git client
path"). So the clone verification cannot pass here regardless of beta access. Re-scope the live gate
to **create → verify `remote_url` returned + token sealed + no leak → delete** (drop the clone), and
move the clone proof to the git-handoff follow-up.

**Recommendation:** adopt **Option A** + the re-scoped live gate, then implement. Until the owner
confirms, this ticket stays in `todo/` unimplemented. Full analysis:
`scratchpad/ticket1-design-tension.md` (this session).

## Overview

[Cloudflare Artifacts](https://www.cloudflare.com/products/artifacts/) is Git-compatible, versioned
repository storage: you create repositories programmatically (or fork/import from a remote), and each
repo gets a stable address `(namespace, repository name)` + an opaque id, a Git **remote URL**, and
repo-scoped read/write **tokens**. Manageable over an account-scoped REST API (and a Workers binding
/ the Git protocol). Status at ticket time: **private beta** (announced 2026-04-16; public beta
targeted ~2026-05). Pricing is usage-based ($0.15/1k operations, $0.50/GB-month, free tiers).

Add support so that, once a Cloudflare account is connected (the `/cf` account/connect model from
commit `b9e1137`), an operator can create and manage their own remote Git repositories through qfs.

**Modeling decision (from the owner, and it is the important one): a repository is a _resource_,
not a procedure.** Creating a repo is *just a write* over the repo path — an `UPSERT`/`INSERT` into
`/cf/artifacts` — exactly like D1/KV are UPSERT-shaped rather than bespoke `CALL`s. Do **not**
introduce `CALL cf.repo_create(...)`-style procedures for the create/fork/delete lifecycle. The
Artifacts surface is a new node under the existing `/cf` driver:

- `/cf/artifacts` — a `RelationalTable` of the account's repositories. `SELECT` lists them
  (columns at least: `namespace`, `name`, `id`, `remote_url`, `created_at`).
- **Create** = `INSERT`/`UPSERT` a row keyed by `(namespace, name)`. The write mints the repo via the
  Artifacts REST API; the returned row carries the **`remote_url`**. Fork/import is the same write
  with an `upstream`/`fork_of` value set (still a write, not a CALL).
- **Delete** = `REMOVE` over `/cf/artifacts/<namespace>/<name>`, gated by qfs's standard
  **irreversible** confirmation (the same gate `CALL github.merge` and other destructive ops use) —
  a preview/describe never deletes.

**Token handling (owner decision): the repo access token is a secret — seal it into the vault, never
return or log it.** The create write seals the minted repo-scoped token into the qfs vault (the same
sealing path `qfs account add` / `networked_credential` use), keyed so a later Git operation can
resolve it; the only thing the write returns to the caller is the non-secret `remote_url`. No token
value ever appears in stdout, JSON, `connect --list`, `qfs dump`, restored JSONL, logs, or errors.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the Artifacts surface lives
  inside the existing `qfs-driver-cf` crate as a new node type; do not add a parallel driver.
- `workaholic:implementation` / `policies/coding-standards.md` — type-driven, compiler-checkable
  Rust; no stringly repo/token plumbing.
- `workaholic:implementation` / `policies/type-driven-design.md` — model repo identity as an
  explicit `(namespace, name)` + opaque `id` type so the human address and the API id cannot be
  mixed (mirrors how D1 uses name-vs-uuid and KV title-vs-id in `b9e1137`); the token is a
  `qfs_secrets::Secret`, never a `String`.
- `workaholic:implementation` / `policies/relational-first-persistence.md` — repos are exposed as a
  relational table with universal verbs, not a bespoke RPC surface (the "it's just an UPSERT"
  decision).
- `workaholic:implementation` / `policies/test.md` — hermetic `MockCfBackend` coverage for
  create/list/delete + verb gating + no-secret-output, plus a live round-trip.
- `workaholic:design` / `policies/defense-in-depth.md` — the minted repo token is sealed in the
  vault and only resolves at the Git boundary; the create path returns the non-secret remote only.
- `workaholic:design` / `policies/vendor-neutrality.md` — the Artifacts REST DTOs stay behind the
  `qfs-driver-cf` boundary; the qfs query surface stays the uniform pipe-SQL, not Cloudflare API
  shapes.
- `workaholic:planning` / `policies/*` — this initiates a new capability against a **private-beta**
  vendor product with usage-based pricing and evolving API; scope it as beta-gated and keep the
  behavior behind the connected-account gate (business/availability/legal grounding).
- `workaholic:operation` / `policies/ci-cd.md` — hermetic tests stay local; the live Artifacts
  round-trip is opt-in and clearly separated (and needs beta access to the connected account).

## Key Files

- `packages/qfs/crates/driver-cf/src/lib.rs` - the `/cf` driver doc + surface currently enumerates
  D1/KV/Queue nodes and `CfNode::parse`; add an `Artifacts` node (`/cf/artifacts`,
  `/cf/artifacts/<namespace>/<name>`) with a `RelationalTable` archetype and the correct
  capability/verb profile (SELECT/INSERT/UPSERT/REMOVE; reject JOIN/aggregate/UPDATE as the
  archetype dictates).
- `packages/qfs/crates/driver-cf/src/registry.rs` - typed resource handles (D1/KV/Queue) added in
  `b9e1137`; add a repositories handle and the `(namespace, name, id)` identity type.
- `packages/qfs/crates/driver-cf/src/backend.rs` - `CfBackend`/`HttpApiBackend` + `MockCfBackend`;
  add the Artifacts REST calls (create, list, get, import/fork, delete, mint-token) as account-scoped
  endpoints, returning the remote URL + (separately) the secret token; extend `MockCfBackend` and a
  `RecordedCall` variant for hermetic tests.
- `packages/qfs/crates/driver-cf/src/applier.rs` - the apply leg; wire the create (UPSERT) and delete
  (REMOVE, irreversible) effects to the backend, sealing the minted token via the vault seam.
- `packages/qfs/crates/qfs/src/cf.rs` - live driver composition; register the Artifacts node for a
  connected cf mount (listing repos may need a live call, mirror the D1/KV discovery fail-closed
  pattern).
- `packages/qfs/crates/qfs/src/commit.rs` / `secret_store.rs` / `vault.rs` - the sealing path
  (`networked_credential`, consent) the token seal reuses; do not invent a second secret store.
- `packages/qfs/crates/driver-github/src/lib.rs` - the **irreversible destructive-op** precedent
  (`merge` is marked irreversible); mirror the gate for repo `REMOVE`, but as a universal verb, not a
  CALL. (`fbc97b8` — per-MAP IRREVERSIBLE plan-time gate, already in this branch — is the newer
  plan-time-gate precedent worth reading too.)
- `docs/guide/cli.md`, `docs/cookbook/*.md` (a Cloudflare/repo recipe), generated `docs/drivers.md`
  (regenerated from the driver describe surface) - operator guidance for creating a repo through qfs.

## Related History

The `/cf` account/connect foundation this builds on landed on this same branch; Artifacts is a fourth
`/cf` resource type alongside D1/KV/Queues.

- [20260707212907-migrate-cloudflare-to-qfs-query-integration.md](.workaholic/tickets/archive/work-20260707-181519/20260707212907-migrate-cloudflare-to-qfs-query-integration.md) - Migrated `/cf` to the account/connect model + live resource discovery (the connected-account gate Artifacts reuses).
- [20260708005659-cf-connect-auto-discover-account-id.md](.workaholic/tickets/archive/work-20260707-181519/20260708005659-cf-connect-auto-discover-account-id.md) - **Shipped** (`c7a71b7`, archived): `qfs connect /cf` is `--at`-optional; the same connected `/cf` mount is the entry point for Artifacts (not a hard code dependency).

## Implementation Steps

0. Merge `origin/main` into the branch first (it is ~6 commits ahead; notably `0d109ca` makes
   gen-docs render from the compiled catalog only, which removes the isolated-`HOME` false-drift
   workaround — the compiled `/cf/artifacts` node WILL appear in `docs/drivers.md` as intended).
1. Confirm the Artifacts REST endpoints and the required API-token scope from the reference
   (https://developers.cloudflare.com/artifacts/api/rest-api/). **Pre-verified (2026-07-08, public
   docs page):** repos — `POST/GET /artifacts/namespaces/:namespace/repos` (create/list),
   `GET/DELETE /artifacts/namespaces/:namespace/repos/:name` (get/delete),
   `POST …/repos/:name/fork` and `POST …/repos/:name/import` (fork/import are separate endpoints —
   the UPSERT-with-`upstream` write maps onto them in the applier); tokens —
   `POST /artifacts/namespaces/:namespace/tokens` (mint, **a separate call after repo create**, so
   the create leg is create-repo → mint-token → seal),
   `GET …/repos/:name/tokens` (list), `DELETE …/namespaces/:namespace/tokens/:id` (revoke — consider
   revoking the sealed token on repo REMOVE). Auth is Bearer with the account API token. Still to
   confirm at implementation: whether these paths sit under the `/accounts/{account_id}` prefix,
   response DTO shapes, and the exact token scope. Record the final paths in `backend.rs` behind the
   driver boundary.
2. `backend.rs`: add the Artifacts calls to `CfBackend` + `HttpApiBackend` (create/list/get/delete/
   import/mint-token), typed request/response DTOs kept private to the crate; the token comes back as
   a `Secret`. Extend `MockCfBackend` + `RecordedCall`.
3. `registry.rs` + `lib.rs`: add the repository identity type `(namespace, name, id)` and the
   `Artifacts` `CfNode` parse arm + archetype/capabilities; SELECT lists, INSERT/UPSERT creates,
   REMOVE deletes (irreversible), other verbs rejected at the parse gate.
4. `applier.rs` + `cf.rs`: wire create to (a) call the REST create, (b) seal the returned token into
   the vault via the existing seam, (c) surface only the `remote_url` in the returned row; wire
   REMOVE to the REST delete behind the irreversible gate. Fail closed with actionable errors when
   the account lacks Artifacts access or the token scope is insufficient.
5. Docs: add a `/cf/artifacts` recipe to `docs/guide/cli.md` + a cookbook article (keep every `qfs`
   recipe line parseable — `crates/test/tests/cookbook_skills.rs` ratchets it), regenerate
   `docs/drivers.md`/skills, and `--check` both (post-merge of main, no isolated-`HOME` needed).
6. Version bumps per `CLAUDE.md`: bump the qfs patch (`crates/qfs/Cargo.toml`) and the plugin
   `version` in all four fields — a new taught surface warrants at least a patch (consider a minor,
   since a whole new `/cf` resource is added).
7. Add the hermetic tests (the gate below) and run the full Quality Gate including the live round
   trip.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `qfs run --json "/cf/artifacts |> limit N"` lists the connected account's repositories with at
  least `namespace`, `name`, `id`, `remote_url` columns (no token column).
- An `INSERT`/`UPSERT` into `/cf/artifacts` with a `(namespace, name)` creates the repo through the
  Artifacts REST API; the returned row carries a usable Git `remote_url`; the minted repo token is
  sealed in the vault and **does not** appear in stdout, JSON, `connect --list`, `qfs dump`, restored
  JSONL, logs, or any error text.
- `REMOVE` over `/cf/artifacts/<namespace>/<name>` deletes the repo and is refused unless the
  irreversible-op confirmation is satisfied; a `describe`/preview of the node performs no deletion.
- Unsupported verbs on `/cf/artifacts` (e.g. `JOIN`, aggregate, `UPDATE` if out of archetype) are
  rejected at the parse gate with a clear error, never half-applied.
- Fail-closed: with no Artifacts access / insufficient token scope, the create/list fails with an
  actionable error naming the missing access, not `unknown_source` and not a false success.

**Verification method** — the commands/tests/probes that prove them:

- Hermetic `MockCfBackend` unit tests: create → returned row has `remote_url`, token sealed into the
  (test) vault and asserted **absent** from the rendered output; list; delete gated by the
  irreversible flag; verb-gating rejections; no-secret-output assertions.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` (never piped through head/tail — observe the exit code), and gen-docs /
  gen-skills `--check` (post-merge of main these are connection-independent; no isolated `HOME`
  needed).
- **Live round-trip (required):** against a Cloudflare account that has Artifacts beta access, create
  a throwaway repo via `/cf/artifacts` UPSERT, `git clone` the returned `remote_url` (auth resolved
  from the vault-sealed token, never typed on argv), confirm the clone succeeds, then `REMOVE` the
  repo. No token value appears in any output during the round-trip.

**Gate** — what must pass before approval:

- All hermetic suites green, clippy clean, fmt clean, gen-docs/gen-skills `--check` in sync.
- The live create → clone → delete round-trip succeeds against a real beta-enabled account with **no
  token leakage**. (Precondition: the connected account has Artifacts private-beta access; if it does
  not, the live gate cannot pass and the ticket is not approvable — surface that explicitly rather
  than approving on hermetic tests alone.)

## Considerations

- **Beta risk.** Artifacts was announced as private beta (public beta targeted ~2026-05); as of
  2026-07-08 the REST API reference is publicly documented, so it may have reached public beta — but
  the connected account's access is still unverified. **Check access early** (a live
  `/cf/artifacts` list against the connected account is the cheapest probe once wiring exists; the
  owner can alternatively probe the REST endpoint via `!`) and stop-and-surface if access is
  missing rather than building past a failing live gate. Endpoints/DTO shapes may still change —
  keep the REST DTOs isolated in `backend.rs` so an API change is a one-file fix, and make the "no
  access" path fail closed with a clear message
  (`packages/qfs/crates/driver-cf/src/backend.rs`).
- **It is a resource, not a procedure.** Resist re-introducing a `CALL` surface for create/fork/
  delete — the whole point is that a repo is a row you write. Fork/import is an UPSERT that carries an
  upstream value, not a separate verb (`packages/qfs/crates/driver-cf/src/lib.rs`).
- **Token is the crown jewel.** The repo token is more sensitive than the account token (it grants
  push). It must be sealed and never rendered; add explicit no-leak assertions and route it only
  through the vault seam (`packages/qfs/crates/qfs/src/vault.rs`, `secret_store.rs`).
- **Git handoff is a likely follow-up.** Actually using the sealed token with qfs's `/git` surface to
  clone/push the created repo is a natural next ticket; this ticket returns the `remote_url` and seals
  the token so that follow-up is unblocked, but does not itself implement the Git client path
  (`packages/qfs/crates/driver-git`).
- Do not regress the D1 `_cf_` internal-table skip / projection-order aliases or the account/connect
  wiring from `b9e1137` (`packages/qfs/crates/qfs/src/cf.rs`).

## Final Report

Development completed as planned for the hermetic and documented `/cf/artifacts` surface. The live
Cloudflare Artifacts round-trip remains owner-gated and was not run in this session.

### Discovered Insights

- **Insight**: The current Cloudflare Artifacts REST docs return `remote` and `token` directly from
  the repository create response, so qfs can create once and seal immediately instead of issuing the
  older ticket's separate mint-token call.
  **Context**: Keeping that response shape isolated in `driver-cf/src/backend.rs` means future
  Artifacts API changes should be confined to the transport DTO boundary.
- **Insight**: Artifacts is account-scoped rather than keyed by one discovered resource segment like
  D1, KV, or Queues.
  **Context**: Registering it as an optional account-level handle in `CfRegistry` preserves the
  existing per-resource maps while still letting `/cf/artifacts/<namespace>/<repo>` address concrete
  repositories.
