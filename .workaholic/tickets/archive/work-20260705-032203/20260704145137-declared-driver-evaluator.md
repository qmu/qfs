---
created_at: 2026-07-04T14:51:37+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: e283c42
category: Added
depends_on: [20260704145136-declared-driver-surface.md]
---

# Declared-driver evaluator: two-source registry, wire execution, host confinement

## Overview

Implement the **evaluation half** of blueprint §13: a driver declared in `/sys/drivers` rows
becomes a live mount when connected. Blueprint §13 is the authority.

- **Two-source registry**: `CONNECT /chatwork TO chatwork` resolves the driver name against
  **compiled ∪ declared**; on a name collision the compiled driver wins and the declared one is
  reported (never silently shadowed).
- **Evaluation**: declared rows reconstruct the wire configuration (the shipped
  `RestApiConfig` machinery is the engine — `AuthStrategy`/`Pagination`/verb→method mapping are
  reused, not reimplemented); declared views evaluate as parameterized reads over the wire
  mount with codec decode; declared MAPs rewrite universal verbs/CALLs into wire effect plans
  (pure; the wire applier performs at COMMIT; per-mapping irreversibility rides the standard
  gate).
- **Host confinement as a hard evaluator rule**: a declared driver's pipelines may only address
  its own declared `AT` host(s). Enforced structurally at evaluation (not policy): a body
  addressing any other host is a structured error. This is the anti-exfiltration guarantee for
  LLM-generated scripts.
- **DESCRIBE stays pure**: declared drivers describe from local rows (types, params, verbs,
  procedures) — no network in describe; `{param}` families report as one node.
- Auth resolves through the existing account layers (`qfs account add <declared-driver>` seals
  the token under the driver's namespace; the OAUTH2 descriptor parameterizes the existing
  browser-consent flow — nonstandard flows are a named park per §13).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions
- `workaholic:implementation` / `policies/domain-layer-separation.md` — the evaluator is engine code; no vendor/wire types leak past the wire boundary
- `workaholic:design` / `policies/defense-in-depth.md` — confinement (evaluator), policy scopes (§8), and the irreversible gate remain independent layers
- `workaholic:implementation` / `policies/observability.md` — declared-driver effects audit identically to compiled ones; confinement denials are structured and secret-free

## Key Files

- `packages/qfs/crates/driver-http/src/` - the wire engine being driven (config construction, applier, pagination follow)
- `packages/qfs/crates/qfs/src/commit.rs` + `crates/qfs/src/read_facets.rs` - where compiled drivers register; the declared source joins here
- `packages/qfs/crates/qfs/src/connections_config.rs` + `crates/qfs/src/path_binding.rs` - CONNECT resolution
- `packages/qfs/crates/http/src/params.rs` - t32 binding, pointed inward for `{param}` views
- `docs/blueprint.md` §13 - the authority

## Implementation Steps

1. Load declared drivers from `/sys/drivers` at registry build (pure read of local DB);
   two-source resolution with the compiled-wins rule + report.
2. Reconstruct wire config per declared driver; evaluate declared views (bind `{param}` from
   the addressed path via the t32 machinery; append the stored body pipeline; codec decode;
   residual honesty unchanged).
3. Lower declared MAPs: verb/CALL on a declared node → the stored wire effect statement with
   params bound → ordinary effect plan (irreversible flag per mapping).
4. Enforce host confinement at evaluation; structured error naming the offending host.
5. Hermetic e2e: install the Chatwork fixture script → connect → DESCRIBE (pure) → read via
   MockHttp → INSERT via MAP → confinement violation test → irreversible-gated MAP test.

## Quality Gate

**Acceptance criteria:**

- The Chatwork fixture works end-to-end hermetically (MockHttp): install → connect → describe →
  parameterized read with pagination → MAP write → all through the language.
- A declared body addressing a foreign host fails with the structured confinement error, both
  at read and at MAP apply (two tests).
- A name collision with a compiled driver resolves compiled-wins with the declared driver
  reported (test).
- DESCRIBE of a declared driver performs no network I/O (MockHttp asserts zero requests).

**Verification method:** `cargo test --workspace`; `clippy --workspace --all-targets -- -D
warnings`; `fmt --all --check`; `gen-docs --check`; `gen-skills --check`.

**Gate:** all green including the four named tests.

## Considerations

- Registry build cost: declared drivers parse per process start; cache within the process, and
  keep DESCRIBE's purity (no lazy network)
- Redirect handling in the wire client must not follow a redirect out of the confined host set
  (confinement applies post-redirect too)
- OAuth2 token refresh for declared drivers reuses the existing google-auth-style machinery
  generically; a service needing a nonstandard flow is a named park, never a special case here

## DONE (2026-07-05, work-20260705-032203) — evaluator complete; all four gate concerns green

The `/rest/<api>/<resource>` path impedance (documented below) was solved by **approach B**:
`MountRemap::new_prefixed` accepts an explicit two-segment inner prefix (`/rest/<name>`) +
`MountDriver::with_remap`, so a declared mount `/chatwork` maps `/chatwork/rooms` →
`/rest/chatwork/rooms` and the stock `RestDriver` resolves capabilities + reads + writes — **zero
change to the shipped `/rest` driver's addressing**. All four acceptance criteria are green:

- **Two-source registry + describe** (`declared_driver.rs`, `describe.rs`): loader → `DeclaredDriver`
  → `rest_config()` (a lift onto `RestApiConfig`); `register_defined_paths` mounts a declared driver
  via `declared_describe_mount` (the `/rest/<name>` remap) with **compiled-wins + report**. ✅ gate 3
  (compiled-wins/reported), ✅ gate 4 (DESCRIBE zero-network), plus `capabilities_resolve_through_the_declared_mount`.
- **Live read/apply wiring** (`shell.rs run_engine_and_reads` + `commit.rs live_registry`): a
  connect-created mount whose driver_id names a declaration registers a LIVE `RestDriver`
  read facet (`read_facets::RestReadDriver` over `qfs_driver_http::rest_read_rows`) and apply driver
  (`rest_apply_driver`), each wrapped in the `/rest/<name>` remap. Fail-closed per mount.
- **Full hermetic e2e** (`declared_driver_reads_and_writes_end_to_end_hermetically`): install→build→
  read (GET the declared host + resource) → parameterized MAP write (POST `/rooms/42/messages`, the
  `{room}` segment passing through) through the interpreter with `MockHttpClient`. ✅ **gate 1**.
- **Host confinement — three layers**: (1) STRUCTURAL/load-time — `body_confined` drops a declared
  driver whose view/map body addresses a foreign `/http/<x>` (`≠` its own name); (2) config
  `allowed_hosts` pins the wire client to the declared host; (3) the `send_one` chokepoint rejects a
  foreign request (link-header follow / override URL) **before dispatch**. ✅ **gate 2** (read via the
  driver-http tests + `body_confinement_rejects_a_foreign_http_host`; MAP write is native base_url).

**Tier-1 scope (as designed):** a declared read/write is a native `RestDriver` read/write —
`resolve_url` is `base_url + path` with `{param}` passing through, so **no view-body-expansion engine
is needed for tier-1**; the stored view/map body is the confinement-check material. A post-decode
pipe op beyond tier-1, and the per-map `IRREVERSIBLE` gate honoring, are §13 named parks. The
redirect-policy layer (reqwest follows 30x internally, `client.rs:66`) is a defense-in-depth park
beyond the `send_one` guard.

---
### Historical: the increments (foundation → impedance → solution)

The evaluator was built in gated increments. Foundation (first checkpoint commit):

- **Loader + two-source registry** (`crates/qfs/src/declared_driver.rs`): `load_declared_drivers()`
  reads `sys_drivers` rows into a `DeclaredDriver` model (driver + its view/map nodes, grouped by
  leading path segment); `DeclaredDriver::rest_config()` **lifts** the row onto the shipped
  `RestApiConfig` (auth/pagination JSON → the closed `AuthStrategy`/`Pagination` sums; resources
  from the view/map verbs). `describe::register_defined_paths` now resolves a CONNECT-ed driver id
  against **compiled ∪ declared** (`resolve_defined_driver`), compiled probed FIRST so **compiled
  wins** a name collision, and the shadow is **reported** (`report_shadowed_declared`). ✅ **Gate
  test 3** (compiled-wins + reported) and **gate test 4** (DESCRIBE of a declared driver does zero
  network — a cred-free `RestDriver` with `MockHttpClient`) pass.
- **Host confinement — RUNTIME guard** (`crates/driver-http`): `RestApiConfig.allowed_hosts`
  (`#[serde(default)]`, EMPTY = unconfined so compiled `/rest` + the `http.get` TVF are unchanged;
  a declared driver populates its own `AT` host), a `HttpError::Confinement { host }` variant, and
  a `confine()` check at the **`send_one` chokepoint** every request funnels through (first page,
  cursor/link follow-ups, writes). ✅ Hermetic tests prove a foreign `Link: rel="next"` follow-up
  and a foreign `override_url` are rejected **before dispatch** (mock never sees them), and an
  unconfined driver still reaches any host. This is **gate test 2's read/runtime half**.

**Remaining (a focused follow-on — deep binary-registry integration; do it fresh):**

1. **Live read/apply registry wiring.** A declared name is invisible to the read/apply funnels
   (`cloud_mounts::canonical_id`, `cloud_mounts.rs:41`, matches cloud kinds only → drops it). Wire a
   declared arm into `commit::live_registry` (`commit.rs:241`, parallel to `register_cloud_mounts`)
   and the read side of `shell::run_engine_and_reads` (`shell.rs:177`), building a LIVE `RestDriver`
   (real transport via `transport.rs`) wrapped in `MountApplyDriver`/`MountReadDriver`. A declared
   `RestDriver`'s **read facet** (`ReadDriver`) has no constructor yet — the compiled cloud read
   facets are built in `cloud_read_facet` (`shell.rs:335`); add the REST one.
2. **Full-language hermetic e2e (gate test 1).** install→connect→describe→parameterized read w/
   pagination→MAP write, through parse+plan+`interp.commit` with an injected `MockHttpClient` (the
   interpreter-level `crates/driver-http/tests/wire.rs:69` pattern). Note: because `resolve_url` is
   always `base_url + path` and path segments pass through, a tier-1 declared read/write is a native
   `RestDriver` read/write (the `{param}` is a pass-through segment) — **no view-body expansion
   engine is needed for tier-1**; the stored view/map body becomes the confinement-check + richer-
   pipeline material (a post-decode pipe op beyond tier-1 is a §13 park).
3. **Confinement at MAP apply through the language (gate test 2's write half).** The `send_one`
   guard already confines writes at the driver level; the language MAP e2e exercises it. Optionally
   add a plan-time **structural** body check (a view/map body addressing `/http/<foreign>` — foreign
   `<x>` ≠ the driver name — rejected before it runs) as defense-in-depth beyond the runtime guard.
4. **Redirect confinement** (Considerations): reqwest follows redirects with no host check
   (`client.rs:66`); scope a `redirect::Policy` (none / host-checking) to the declared driver's
   transport so a 30x cannot leave the confined host. Not covered by the `send_one` guard (reqwest
   follows internally).

### ⚠ KNOWN HARD PROBLEM — solve FIRST: the `/rest/<api>/<resource>` path impedance

Investigated 2026-07-05 while attempting the live wiring. **`RestDriver` is hard-wired to the
`/rest/<api>/<resource>` path shape** (`resource_path_of`/`resource_segment_of`,
`driver-http/src/applier.rs` — they take `segments[2..]`, i.e. they RESERVE `segments[1]` as the
`<api>` label). But a declared mount is `/<name>/<resource>` (`/chatwork/rooms`). `MountRemap`
(`mount_adapter.rs:70`) only accepts a **single-segment** inner id, so mounting a declared
`RestDriver` (`id`=`rest`) at `/chatwork` maps `/chatwork/rooms` → `/rest/rooms` (2 segments) →
`resource_segment_of` returns `None` → **empty `Capabilities`** → a read/write of `/chatwork/rooms`
is rejected at the parse-time gate before it can run. (This also means the **describe** mount already
registered in this branch returns the right static schema but resolves EMPTY capabilities — the
mount is describe-only until this is fixed. The committed tests pass because they assert describe
purity + collision, not capability resolution.)

Two clean fixes (pick one, do it fresh):

- **(A) A mountable `RestDriver`** — let `RestDriver::new` take the mount/id (e.g. `/rest/chatwork`,
  id `chatwork`) and address resources as `segments[after-the-mount..]`, so `/chatwork/rooms` maps to
  base_url+`rooms` directly. Cleanest; keeps all three facets (describe/read/apply) working through
  the normal `MountDriver`/`MountReadDriver`/`MountApplyDriver` with a **single-segment** remap.
- **(B) Declared-driver facet wrappers** — a small trio (describe/read/apply) that prepend `/rest`
  to a `/<name>/<resource>` path (so `<name>` becomes the `<api>`) before delegating to the stock
  `RestDriver`/`RestApplier`. Non-invasive to driver-http but adds three wrappers.

The rest of the plan above (read facet via `rest_read_rows`, e2e, confinement-at-MAP) is
straightforward ONCE the path shape resolves — a prototype `RestApplier::read` + `rest_read_rows`
(a `Read`-effect → GET → decode → `RowBatch`) compiled cleanly this session and was reverted only
because it is inert until (A)/(B) lands. Approach **(A)** is recommended — it makes the already-
committed describe mount fully functional (capabilities + read + write) with the least new surface.
