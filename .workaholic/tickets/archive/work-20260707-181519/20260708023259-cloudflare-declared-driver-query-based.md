---
created_at: 2026-07-08T02:32:59+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Config]
effort:
commit_hash: 63cfab7
category: Added
depends_on:
mission:
---

# Ship a query-based (declared) Cloudflare driver at `/cloudflare`, like chatwork

## Overview

qfs has a **declared-driver** surface (blueprint §13): an integration is defined in the query
language itself — `CREATE DRIVER … AT '<url>' AUTH …`, `CREATE TYPE`, `CREATE VIEW /x AS /http/… |>
DECODE json`, `CREATE MAP INSERT /x AS INSERT INTO /http/… VALUES ({…})` — which desugars into
`/sys/drivers` rows and lifts the shipped `qfs-driver-http` `RestApiConfig` (a lift, not a new
engine). Installing such a script is an ordinary preview/commit; `CONNECT … TO <name>` evaluates it.
The chatwork script is the canonical "LLM-generated integration" example (it lives in the parser
tests and `docs/blueprint.md`, not as a shipped `examples/*.qfs` — `cloudflare.qfs` will be the
first shipped declared-driver example script).

Make Cloudflare available the same declarative way: ship a `cloudflare.qfs` declared driver mounted
at **`/cloudflare`**, covering a **broad slice of Cloudflare's REST API** (zones, DNS records, KV,
Queues, Artifacts repositories, and other account/token-scoped resources), so users and agents can
extend Cloudflare coverage without new compiled Rust.

**Locked decisions (from the owner):**

1. **Additive, not a replacement.** The compiled `qfs-driver-cf` (`/cf`) stays exactly as is. **D1
   is NOT moved to a declared driver** — D1 reuses qfs's SQL planner (WHERE/JOIN/aggregate pushdown,
   catalog introspection, injection-safe param binding), which a declared REST view cannot express.
   The declared driver covers the plain-REST Cloudflare surface only.
2. **Separate mount `/cloudflare`.** The two-source registry resolves `CONNECT … TO <name>` against
   *compiled ∪ declared*, and **compiled wins a name collision** — a declared driver named `cf`
   would be silently shadowed by the compiled `/cf`. So the declared driver is a distinct mount
   (`/cloudflare`), coexisting with `/cf`.
3. **Broad coverage.** Not just KV/Queues mirrors — include the wider Cloudflare REST surface
   (zones, DNS, etc.). The shipped script also seeds the pattern so a user can add more resources.
**Dependency status (verified 2026-07-08, pre-drive refinement): tier-2 declared wire execution
has LANDED and is already in this branch.** `66669aa` (loader/two-source registry/host confinement),
`2ca3a04` (evaluator: registry, live read/write, confinement), `0c825fe` (transport honesty),
`fdfe534` (tier-2 view-body evaluation), `03bb96a` (tier-2 MAP VALUES eval), and `fbc97b8` (per-MAP
IRREVERSIBLE plan-time gate) are all ancestors of this branch's HEAD. The live `/cloudflare` read is
therefore a **required** gate item, not a deferrable concern. Additionally, `origin/main` is ahead
of this branch and carries `DeclaredMount` + `CONNECT … TO <driver> SECRET '<ref>'` lazy secret-ref
resolution in `declared_driver.rs` (+120 lines, PR #27/#28) — **merge `origin/main` into the branch
before starting**, since step 3 (connect + auth) builds on exactly that seam.

4. **Account-scoped paths are explicit, not magic.** Cloudflare's REST paths are account-scoped
   (`/accounts/{id}/…`). Rather than injecting a hidden account id, model the mount so the account is
   an explicit `{account}` path segment (`/cloudflare/accounts/{account}/kv/{ns}/keys`), mirroring
   both the Cloudflare API shape and qfs's path-shaped philosophy — `{param}` segments already bind
   at read time from the concrete mount path. Token-scoped resources (e.g. `/cloudflare/zones`) need
   no account segment. The account id the operator uses is the one `qfs connect /cf` now
   auto-discovers (see `20260708005659`), visible in `qfs connect --list`.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the integration is a shipped
  `.qfs` declaration (data) beside the other example scripts, plus loader/eval in the existing
  declared-driver modules; no new driver crate.
- `workaholic:implementation` / `policies/coding-standards.md` — any Rust touched (loader/eval gaps)
  stays type-driven; the auth descriptor and paths are declarative data, never stringly secrets.
- `workaholic:implementation` / `policies/vendor-neutrality.md` — **the core reason for this
  ticket**: Cloudflare expressed as declarative config over the generic `/http` wire primitive, so
  vendor coverage grows without vendor-specific Rust.
- `workaholic:implementation` / `policies/test.md` — hermetic install/desugar/describe/confinement
  tests + the cookbook parse-check ratchet; live read opt-in.
- `workaholic:design` / `policies/defense-in-depth.md` — **host confinement** is an evaluator rule:
  a declared driver's bodies may address only its own `/http/cloudflare/…` host, so the script is
  structurally unable to read Cloudflare and exfiltrate elsewhere; the token lives in the account
  layer, never in the script.
- `workaholic:design` / `policies/vendor-neutrality.md` — keep the qfs query surface uniform; the
  Cloudflare REST shapes stay in the declaration, not in the grammar.
- `workaholic:operation` / `policies/ci-cd.md` — hermetic parse/describe/install runs locally; the
  live `/cloudflare` read is opt-in and gated on declared wire execution being available.

## Key Files

- `packages/qfs/crates/skill/assets/examples/` - shipped example `.qfs` scripts (drive/git/github/
  mail live here); add `cloudflare.qfs` (the `CREATE DRIVER cloudflare AT
  'https://api.cloudflare.com/client/v4' AUTH BEARER` + the `CREATE VIEW/MAP` resource set).
- `packages/qfs/crates/qfs/src/declared_driver.rs` - loads `/sys/drivers` rows into a
  `DeclaredDriver` and lifts `RestApiConfig`; verify a broad multi-resource Cloudflare driver with
  `AUTH BEARER` and `{param}` (incl. `{account}`) paths loads + describes correctly; extend if a gap
  surfaces (e.g. pagination for list endpoints).
- `packages/qfs/crates/exec/src/declared.rs` - blueprint §13 tier-2 declared-view **body
  evaluation** (the live read path) — **landed** (`fdfe534`, `03bb96a`; `eval_view_body` /
  `eval_map_body` / `match_template` exist on this branch). Confirm a Cloudflare view body
  (`/http/cloudflare/… |> DECODE json |> EXPAND …`) executes; only if a real shape gap surfaces
  (e.g. pagination) record it precisely.
- `packages/qfs/crates/qfs/src/describe.rs` - two-source (compiled ∪ declared) describe
  registration; ensure `/cloudflare` registers as declared while `/cf` stays compiled (no collision,
  compiled `cf` unaffected).
- `packages/qfs/crates/parser/…` + the surface desugar for `CREATE DRIVER/TYPE/VIEW/MAP` - the
  grammar that turns the script into `/sys/drivers` rows (the `chatwork` parser tests are the
  precedent).
- `packages/qfs/crates/qfs/src/{dump,restore,sys}.rs` - `/sys/drivers` dump/restore already carries
  declared drivers; confirm the Cloudflare driver round-trips with no token in the rows.
- `docs/cookbook/*.md` (+ generated skills) and `docs/blueprint.md` (already carries the chatwork
  declared example) - operator guide for installing/connecting the declared Cloudflare driver; every
  `qfs`/DDL recipe line is parse-checked by `crates/test/tests/cookbook_skills.rs`.

## Related History

The compiled `/cf` this coexists with was built and account/connect-migrated on this branch; the
declared-driver machinery this uses is the chatwork line.

- [20260707212907-migrate-cloudflare-to-qfs-query-integration.md](.workaholic/tickets/archive/work-20260707-181519/20260707212907-migrate-cloudflare-to-qfs-query-integration.md) - Migrated compiled `/cf`; its decision "keep the compiled driver because D1 needs SQL planning" is exactly why this ticket is additive (declared covers REST only, not D1).
- [20260708005659-cf-connect-auto-discover-account-id.md](.workaholic/tickets/archive/work-20260707-181519/20260708005659-cf-connect-auto-discover-account-id.md) - Auto-discovers the Cloudflare account id; that id is the `{account}` segment operators use in `/cloudflare/accounts/{account}/…` paths.
- [20260708013532-cf-artifacts-repositories-as-a-resource.md](.workaholic/tickets/todo/a-qmu-jp/20260708013532-cf-artifacts-repositories-as-a-resource.md) - Artifacts as a `/cf` resource (compiled); the declared driver can additionally expose Artifacts repo REST as `/cloudflare/…` views — coordinate so the two surfaces are complementary, not duplicative.

## Implementation Steps

0. Merge `origin/main` into the branch first (it is ~6 commits ahead: `DeclaredMount` +
   `CONNECT … SECRET '<ref>'` in `declared_driver.rs`, and `0d109ca` which renders gen-docs from the
   compiled catalog only — this also removes the need for the hermetic-`HOME` gen-docs workaround).
1. Author `cloudflare.qfs`: `CREATE DRIVER cloudflare AT 'https://api.cloudflare.com/client/v4' AUTH
   BEARER`, then `CREATE VIEW`/`CREATE MAP` for a broad resource set — token-scoped (`/cloudflare/
   zones`, `/cloudflare/zones/{zone}/dns_records`) and account-scoped (`/cloudflare/accounts/
   {account}/storage/kv/namespaces`, `/cloudflare/accounts/{account}/queues`, `/cloudflare/accounts/
   {account}/artifacts/…`). Add `CREATE TYPE`s where a stable row shape helps. Keep every wire body
   confined to `/http/cloudflare/…`.
2. Install-path check: preview then commit the script; confirm it lands `/sys/drivers` rows and
   `DESCRIBE /cloudflare/…` renders cred-free (zero network) via the two-source registry, with the
   compiled `/cf` still present and unshadowed.
3. Connect + auth: `CONNECT /cloudflare TO cloudflare` bound to the stored Cloudflare token (the
   account layer, same vault the `cf` account uses; post-merge, the `CONNECT … SECRET '<ref>'`
   lazy-resolution path from main is available if a secret reference fits better); verify no token
   appears in the script, rows, dump, or restore.
4. Live read (**required** — tier-2 wire execution is on this branch): `SELECT` over a token-scoped
   view (`/cloudflare/zones`) and an account-scoped view; confirm `{account}`/`{zone}` bind from the
   mount path and the body executes. Only if a genuine evaluator shape gap surfaces (e.g.
   pagination), record the precise gap as a concern (do not fake a pass).
5. Docs: a cookbook article for the declared Cloudflare driver (install → connect → query), keeping
   recipes parseable; regenerate docs/skills and `--check`. **Note (post-merge):** gen-docs renders
   from the compiled catalog only (`0d109ca`), so the declared `/cloudflare` will NOT and should not
   appear in `docs/drivers.md` — the cookbook article + generated skill are its doc surface, and the
   isolated-`HOME` workaround is no longer needed.
6. Version bumps per `CLAUDE.md` (qfs patch + plugin `version` in all four fields — a new taught
   surface).
7. Add hermetic tests and run the Quality Gate.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Installing `cloudflare.qfs` (preview then commit) succeeds and lands `/sys/drivers` rows; no
  network I/O occurs during install or `DESCRIBE`.
- `qfs describe /cloudflare/zones` (and an account-scoped path) renders the declared surface
  cred-free; the compiled `/cf` describe/plan is unchanged and unshadowed (two-source, compiled
  wins its own name).
- Host confinement holds: a declared view whose body addresses a non-`/http/cloudflare` host is
  rejected at load (structural check), not at request time.
- No token value appears in the `.qfs` script, `/sys/drivers` rows, `qfs dump`, restored JSONL,
  `connect --list`, logs, or any error text.
- `{account}`/`{zone}`/`{ns}` path params bind from the concrete mount path; a missing required
  segment is a clear usage error, never a silent wrong-endpoint fetch.
- Cookbook/DDL recipes parse-check green (`crates/test/tests/cookbook_skills.rs`).

**Verification method** — the commands/tests/probes that prove them:

- Hermetic tests: parse+desugar+load+describe `cloudflare.qfs` over a mock HTTP client (zero
  network); confinement-rejection test; two-source registration test (compiled `/cf` present +
  declared `/cloudflare` added); dump/restore round-trip with no-token assertion.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` (never piped through head/tail — observe the exit code), gen-docs /
  gen-skills `--check` (post-merge of main these are connection-independent; no isolated `HOME`
  needed).
- **Live read (required):** with the Cloudflare token in the vault, a read-only `SELECT` over
  `/cloudflare/zones` returns live rows and an account-scoped view resolves `{account}`, with no
  token leak. Tier-2 wire execution is already on this branch, so this is not deferrable on that
  dependency; only a newly-discovered, precisely-named evaluator gap (e.g. pagination shape) may
  defer it as a concern.

**Gate** — what must pass before approval:

- Hermetic install/describe/confinement/two-source/dump-restore suites green; clippy clean; fmt
  clean; gen-docs/gen-skills `--check` in sync; compiled `/cf` regression-free.
- The live `/cloudflare` read succeeds (token-scoped view at minimum). A deferral is acceptable
  only for a newly-found evaluator gap named precisely (file + shape), not the already-landed
  tier-2 dependency.

## Considerations

- **Declared wire execution has landed** (`fdfe534`, `03bb96a`, `fbc97b8`, `0c825fe` — all in this
  branch's history). The former "tier-2 still landing" blocker is resolved; the full
  install/describe/confinement/connect/live-read scope is deliverable in one pass
  (`packages/qfs/crates/exec/src/declared.rs`).
- **Do not duplicate the compiled surface confusingly.** `/cf` (compiled: D1 SQL, KV, Queues) and
  `/cloudflare` (declared: broad REST) coexist by design; the docs must make clear which to use for
  what (D1 SQL → `/cf`; broad REST / user-extensible → `/cloudflare`) so operators are not lost
  between two Cloudflare mounts.
- **Coordinate with the Artifacts ticket** (`20260708013532`): Artifacts repos can be a compiled
  `/cf/artifacts` resource *and*/or declared `/cloudflare/…` views. Decide, at implementation, which
  is authoritative to avoid two divergent create paths (`packages/qfs/crates/driver-cf/`).
- **Account-scoped vs token-scoped endpoints.** Token-scoped resources (zones) declare trivially;
  account-scoped ones must carry `{account}`. Keep that explicit in the mount path rather than
  hiding it, so a wrong/absent account is a visible path error.
- **Beta surfaces.** If Artifacts (private beta) views are included, keep them isolated in the
  script so a beta API change is a one-declaration edit, and fail closed when the account lacks
  access.
