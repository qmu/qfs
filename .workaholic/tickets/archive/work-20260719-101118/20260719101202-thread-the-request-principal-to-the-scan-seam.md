---
created_at: 2026-07-19T10:12:02+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission: a-request-resolves-to-a-principal-the-query-path-can-read
---

# Thread a request-derived principal to the scan seam and the policy gate

Satisfies mission acceptance items 1-5: **"Who am I" answerable + not-signed-in first-class**,
**principal reachable on the scan path**, **policy gate evaluates the resolved actor not
`anonymous()` (both directions)**, **fail-closed preserved & pinned by a test**, and
**answer readable as data through the one engine on the `/sys` closed set, credential-free**.

Developer ruling (SEAM SHAPE = A): **change the core read trait.** Make
`ReadDriver::scan` (`crates/exec/src/read.rs:48`) carry an explicit request/principal
context so EVERY driver receives the principal. Blast radius across all driver impls is
accepted — qfs is experimental, hard breaks are correct, NO backward-compat/migration shims.
The explicit arg is chosen precisely so fail-closed is pinned by a test.

## Design decisions (settled against source; NOT to be re-deferred)

- **`RequestContext` shape + home.** Define `RequestContext` in **`qfs-core`** (the hub every
  read-path crate already reaches; `qfs-exec` depends on `qfs-core`, `qfs-server` cannot be a
  dep of `qfs-exec`). Fields: `principal: Principal`, where `Principal` is
  `Anonymous` | `User { id: String }` (owned, secret-free). `RequestContext::anonymous()` is the
  fail-closed default. Keep it minimal — roles/groups/memberships are resolved into the server's
  `DecisionContext` on the policy side, not carried through the scan seam. The scan seam only
  needs *who is asking*; the /sys/whoami face reads it.
- **Trait change.** `async fn scan(&self, scan: &ScanNode, ctx: &RequestContext) -> Result<RowBatch, CfsError>;`
  Every `impl ReadDriver` in the workspace adds the param (most ignore it as `_ctx`):
  `crates/qfs/src/{claude,server_face,shell,markdown,mount_adapter,read_facets,serve_builtins,sys,transform,type_catalog}.rs`,
  `crates/exec/src/shell/complete.rs`, `crates/http/src/tests.rs` (the delegating fake). Re-verify
  the full list at drive time with `grep -rln "impl ReadDriver for" crates/*/src`.
- **Thread through the executor.** `execute_read`/`execute_read_with`
  (`crates/exec/src/exec.rs:46,60`) take `ctx: &RequestContext` and pass it to
  `driver.scan(scan, ctx)` (`:84`). Update `block_on_read`/`block_on_read_with` and the
  `qfs-exec` re-exports (`crates/exec/src/lib.rs`). Production callers:
  `crates/http/src/handler.rs:136`, `crates/watchtower/src/watcher.rs:153`, and the shell/REPL
  path. Non-HTTP callers pass `RequestContext::anonymous()` (the CLI/local path has no session —
  the not-signed-in answer is first-class, NOT a silent fallback to the sole user).
- **`/sys/whoami` — the closed-set variant (NOT a side-channel endpoint).** Add
  `SysNode::Whoami` (`crates/driver-sys/src/schema.rs`): `from_segment("whoami")`,
  `sys_node_schema` = credential-free columns `signed_in: Bool`, `user: Text` (nullable when
  anonymous) — NO credential column, matching the `/sys/connections` redaction contract
  (`schema.rs:41-46`), and `sys_node_capabilities` = `Select` only. The `/sys` ReadDriver facet
  (`crates/qfs/src/sys.rs`) emits the Whoami row **from `ctx`** (the resolved principal), which is
  precisely why the seam must carry it. `DESCRIBE /sys/whoami` returns the stable schema with no DB.
- **Policy gate evaluates the resolved actor.** On the HTTP write-lowering path
  (`crates/http/src/policy.rs:76` → `qfs_server::evaluate`), replace the back-compat
  `evaluate(policy, plan)` with `evaluate_with_context(policy, plan, &ctx)` where `ctx` is a
  `DecisionContext` built from the request's resolved principal (map `RequestContext::User{id}` →
  `DecisionContext::for_user(id)`; `Anonymous` → `DecisionContext::anonymous()`). The binary maps
  the session cookie → `UserId` via the shipped `qfs_session::authenticate(cookie, store)` (today
  called only in the OAuth face, `qfs/src/oauth.rs:232,262`) — reuse it on the query path. Roles
  resolve later (t57/t58); this ticket supplies the *user* axis, which is enough to prove both
  directions.

## Proofs required (all as hermetic tests, no live creds)

- **Both directions on the gate.** A `FOR <user>` narrowed rule contributes with a principal
  present and contributes nothing under anonymous — one test each.
- **Fail-closed pinned.** A test that would FAIL if the anonymous default ever widened: anonymous
  ⇒ no user/roles/groups/memberships ⇒ default-deny holds.
- **`/sys/whoami` structural test.** `DESCRIBE /sys/whoami` yields exactly the credential-free
  schema (no secret column); a scan under a User ctx yields `signed_in=true, user=<id>`; a scan
  under anonymous yields `signed_in=false, user=null`.

## Policies

**設計 / `workaholic:design`**
- `access-control` — "Define the authorization layer once." The seam threads ONE principal that
  both the /sys read face and the policy gate read; no per-face divergence, no second check.
  A trail is not a bypass: resolution happens under the caller's principal.
- `defense-in-depth` / least-privilege — the anonymous default stays the most restrictive
  starting point; threading a principal must widen nothing (the pinned fail-closed test).
- `data-sovereignty` — `/sys/whoami` carries no credential column; the schema is the boundary.

**実装 / `workaholic:implementation`**
- `machine-checkable-domain` — an explicit `ctx` arg (not an ambient/thread-local) makes the
  fail-closed contract a compile-time + test-pinned fact, which is the stated reason for shape A.
- `reachability` — the answer is data through the one engine (`/sys` closed set), reachable by
  screen, query, console, and HTTP alike.
- `anti-corruption-structure` — `RequestContext` is owned, secret-free, vendor-free; no session
  or cookie type crosses the scan seam.

**House rules (`CLAUDE.md`)**
- Hard break, no back-compat shim (experimental). gen-docs/gen-skills anti-drift must stay green
  (the trait doc + drivers.md render from the binary). Plugin re-version only if a skill-taught
  CLI surface changes (it does not here).

## Quality Gate

**Acceptance criteria.** (1) A request with a live session resolves to a named principal on the
query path; a request with none resolves to an explicit not-signed-in answer (not an error, not a
silent fallback). (2) `ReadDriver::scan` carries the principal; every driver receives it. (3) The
policy gate evaluates the resolved actor — proven both directions. (4) Fail-closed preserved,
pinned by a test that fails if the default widens. (5) `/sys/whoami` returns the answer as
credential-free data through the one engine.

**Verification method.** The hermetic tests above, plus `DESCRIBE /sys/whoami` and a real
`qfs run -e '/sys/whoami'` under both an anonymous and a session-bearing context.

**Gate that must pass.** `cargo fmt` on every touched crate before commit, then
`cargo build --workspace`, `cargo test --workspace` (per-crate if disk-tight),
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
`cargo run -p xtask -- gen-docs --check`, `gen-skills --check`, `check-migrations` — all exit 0.

## Invariants (must not break)

- `identity::Role` remains NOT a grant (this ticket does not touch it).
- Do NOT settle super-admin vs project-admin; do NOT answer "what may I administer".
- The fail-closed / least-privilege default must never widen.
