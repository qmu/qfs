---
created_at: 2026-07-07T21:29:07+09:00
author: a@qmu.jp
type: refactoring
layer: [Infrastructure, Config]
effort:
commit_hash: 418ffea
category: Changed
depends_on:
mission:
---

# Migrate Cloudflare live configuration to qfs account/connect state

## Overview

The Cloudflare `/cf` integration is currently a compiled native driver whose live D1/KV/Queues access is configured from process-global `CF_*` environment variables. Migrate the operator-facing configuration to qfs's query/account/connect model so Cloudflare credentials and resource bindings can be declared, stored, listed, dumped, restored, and driven through qfs state instead of ad hoc environment setup.

The migration should preserve the native Cloudflare driver semantics where they are materially richer than generic REST mapping: D1 reuses qfs SQL planning, KV exposes the key/value table shape, and Queues expose append/log semantics. If a declared-driver/query-defined replacement can faithfully express those semantics, document and implement that path; otherwise wire the existing compiled `cf` driver to qfs account and path-binding state.

## Related History

- `b9d2ad8 Implement remaining qfs live runtime features` - introduced or completed live runtime wiring that now exposes `/cf` through `CF_*` environment configuration.
- `61f696c Add CREATE ACCOUNT statement and /sys/accounts` - established the account consent model that should store Cloudflare credentials.
- `f67ef53 Converge the split-brain sql registries on the qfs-connect binding` - precedent for moving a source from env-only configuration into the path-binding registry.
- `474f583 Add FAQ CLI-surface anti-drift test` - docs/skill surfaces must stay true when CLI/account guidance changes.
- Current branch work fixed two live `/cf` validation defects: skipping `_cf_` D1 internal tables during introspection, and preserving D1 JSON projection order with `cN` aliases.

## Key Files

- `packages/qfs/crates/qfs/src/cf.rs` - current Cloudflare live-driver composition reads `CF_ACCOUNT_ID`, `CF_API_TOKEN`, `CF_D1_DATABASES`, `CF_KV_NAMESPACES`, and `CF_QUEUES`.
- `packages/qfs/crates/qfs/src/shell.rs` - registers live read facets, including direct `/cf` env-backed registration and connect-created cloud mounts.
- `packages/qfs/crates/qfs/src/commit.rs` - registers live apply drivers, including env-backed `/cf` and mount-bound cloud apply drivers.
- `packages/qfs/crates/qfs/src/account.rs` - stores `cf` account credentials in the vault through `qfs account add cf <label>`.
- `packages/qfs/crates/qfs/src/cloud_mounts.rs` - loads connect-created cloud mounts from `path_binding`.
- `packages/qfs/crates/qfs/src/path_binding.rs` - stores defined paths created by `qfs connect` / `CONNECT`.
- `packages/qfs/crates/qfs/src/describe.rs` - registers compiled and connected driver surfaces for `qfs describe`.
- `packages/qfs/crates/driver-cf/src/lib.rs` - native Cloudflare driver surface for D1/KV/Queues.
- `docs/guide/cli.md` and `docs/cookbook/automation.md` - currently document `/cf` as `CF_*` env configured.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` - keep the migration inside existing composition roots and driver crates instead of adding a parallel configuration system.
- `workaholic:implementation` / `policies/coding-standards.md` - preserve type-driven, compiler-checkable Rust structure and avoid stringly credential plumbing.
- `workaholic:implementation` / `policies/type-driven-design.md` - model Cloudflare account/resource bindings explicitly so D1/KV/Queue registration cannot mix incompatible identifiers.
- `workaholic:implementation` / `policies/test.md` - cover the migration with unit tests and at least one live read-only validation path.
- `workaholic:design` / `policies/defense-in-depth.md` - credentials must remain sealed in the vault and only resolve at the live driver boundary.
- `workaholic:design` / `policies/vendor-neutrality.md` - keep Cloudflare-specific behavior behind the driver boundary and avoid leaking vendor API details into qfs query syntax unnecessarily.
- `workaholic:operation` / `policies/ci-cd.md` - keep hermetic tests runnable locally, with live Cloudflare checks clearly separated and opt-in.

## Quality Gate

### Acceptance Criteria

- A Cloudflare API token can be stored with `qfs account add cf <label>` and later used by `/cf` without setting `CF_API_TOKEN`.
- A Cloudflare account/resource binding can be declared through `qfs connect` or the equivalent query-language `CONNECT`/state path, rather than only through `CF_ACCOUNT_ID`, `CF_D1_DATABASES`, `CF_KV_NAMESPACES`, and `CF_QUEUES`.
- Existing `/cf/d1/<db>/<table>`, `/cf/kv/<namespace>`, `/cf/kv/<namespace>/<key>`, and `/cf/queue/<queue>` path semantics remain compatible, or a documented migration path is provided with tests.
- `qfs account list`, `qfs connect --list`, `qfs dump`, and `qfs restore --commit` reflect the Cloudflare account/binding metadata without printing or exporting the token value.
- The env-var path is either retained as a documented compatibility fallback or removed with docs/tests updated in the same change.

### Verification Method

- Run `cargo test -p qfs-driver-cf`.
- Run focused qfs composition tests covering `cf` account/path-binding registration without reading `CF_API_TOKEN`.
- Run CLI/account tests covering `qfs account add cf`, `qfs account list`, `qfs connect ... --driver cf --account <label>`, and `qfs connect --list` metadata surfaces.
- Run docs anti-drift checks if CLI/docs text changes: `cargo run -p xtask -- gen-docs --check` and `cargo run -p xtask -- gen-skills --check`.
- With a live Cloudflare token available, run read-only probes without `CF_API_TOKEN` in the environment:
  - D1: `qfs run --json "/cf/d1/<db>/<table> |> select <columns> |> limit 1"`
  - KV: `qfs run --json "/cf/kv/<namespace> |> limit 10"`

### Gate

- Hermetic tests for the driver and qfs composition are green.
- The live read-only D1 and KV probes succeed using the stored qfs account/binding state, not process-global `CF_API_TOKEN`.
- No token value appears in stdout, logs, JSON error envelopes, account listings, dumps, restored JSONL, or git diffs.

## Implementation Steps

1. Inventory the current env-backed `/cf` composition in `cf.rs`, `shell.rs`, and `commit.rs`, including direct registration and connect-created cloud mount registration.
2. Decide the target shape:
   - Prefer using the existing compiled `qfs-driver-cf` with vault-backed `cf/<label>` credentials and path-binding resource metadata.
   - Only replace it with a declared-driver/query-defined REST mapping if D1 SQL planning, KV table semantics, and Queue append/log semantics can be represented without behavioral loss.
3. Extend the Cloudflare binding model so it can carry the Cloudflare account id and resource lists from qfs state. Avoid putting the API token, account id, or resource list in argv when a secret/reference field is more appropriate.
4. Make read registration (`shell.rs`) and apply registration (`commit.rs`) build Cloudflare drivers from qfs account/path-binding state. Keep missing credentials fail-closed with actionable errors.
5. Preserve or deliberately migrate the existing env-var compatibility path. If retained, document precedence between qfs state and `CF_*`.
6. Update docs and generated skills so operator guidance says how to store a Cloudflare token and bind D1/KV/Queue resources through qfs.
7. Add tests for stored-account resolution, connected Cloudflare mount registration, dump/restore metadata safety, and no-secret output.
8. Run the Quality Gate.

## Considerations

- Do not regress the native D1 catalog behavior fixed in this branch: Cloudflare internal `_cf_` tables must not break startup, and D1 JSON row decoding must preserve projection order.
- D1 databases are discovered by Cloudflare's `uuid` field, while KV namespaces use `id`; the binding model should name those fields clearly.
- Cloudflare Queues were not present in the live validation account, so Queue coverage may need hermetic tests unless a live queue is provisioned.
- Cloudflare KV rejects key-list limits below 10; user-facing validation or docs should avoid suggesting smaller KV list limits.
- Account-scoped Cloudflare API tokens may verify through `/accounts/<account>/tokens/verify`; the generic `/user/tokens/verify` endpoint may reject them.
