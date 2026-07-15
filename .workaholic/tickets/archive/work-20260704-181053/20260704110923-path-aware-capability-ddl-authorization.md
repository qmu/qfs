---
created_at: 2026-07-04T11:09:23+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash: 013d52e
category: Changed
depends_on:
---

# Path-aware capabilities: fine-grained DDL/DML authorization (blueprint §8)

> **Design settled (2026-07-04, Fable session): blueprint §8 "Policy grants become path-aware".**
> Key finding that shrank this ticket: `CREATE POLICY … ALLOW <verbs> [ON <driver>] [FOR
> <subject>] [AT <path-glob>] [WHERE <cond>]` **already parses** (t57) — the language surface
> exists and needs **no grammar change**. What remains is purely the enforcement gap: the runtime
> grant tuple grows an optional path scope, `(driver, verb, Option<PathScope>)`, matched
> prefix/glob against the effect target; unscoped grants keep matching any path (additive — no
> existing policy narrows silently). This ticket is now **implementation-only (Opus-class)**;
> follow blueprint §8 as the authority. ADR 0009 §6 references below resolve to blueprint §8
> (the ADR pile is retired).

## Overview

Make the runtime authorization layer **path-aware** so a server policy can grant DML on a table
while denying DDL on its catalog node — the "data-only" / "read-only" connection the ADR 0009 §6
deny/allow matrix specifies. This is the one remaining, deliberately-separated piece of the SQLite
DBMS work (ticket `20260704001233`): that ticket delivered create/drop/list tables through the
language, and found that fine-grained DDL authorization is **not expressible today** and is a
cross-cutting runtime security change, not SQL-driver work.

**Why it is blocked today (the finding that motivated this ticket):**
`qfs_runtime::CapabilitySet::allows(target, kind)` keys a grant on `(driver_id, verb_label)` **only,
never the path** (`crates/runtime/src/caps.rs`). So a DDL effect (`INSERT INTO /sql/<conn>` — create
table) and a DML effect (`INSERT INTO /sql/<conn>/<table>` — insert row) are **both** `(sql, INSERT)`
and cannot be told apart. Coarse authorization already works — a policy denying `(sql, INSERT)`
blocks *both* — but the data-only distinction (DML yes, DDL no) needs path scope on the grant.

Note the current CLI scope: one-shot `qfs run --commit` uses `CapabilitySet::allow_all()`
(`crates/qfs/src/commit.rs`), so policy enforcement is a **server-side** concern (endpoints / jobs /
triggers running under a `CREATE POLICY`). This ticket is therefore about the server authorization
path, not the CLI.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions
- `workaholic:design` / `policies/access-control.md` — the mechanism must stay proportionate: add
  path scope as an *additive* refinement, not a whole new policy engine
- `workaholic:design` / `policies/defense-in-depth.md` — the path-scoped capability check is one
  independent layer, composing with (not replacing) the irreversible-commit gate
- `workaholic:design` / `policies/admin-isolation.md` — DDL/provisioning is the administrative
  surface a data-level grant must not reach
- `workaholic:implementation` / `policies/type-driven-design.md` — model a grant's optional path
  scope so an unscoped grant (match-any) and a scoped grant are distinct in the type
- `workaholic:operation` / `policies/observability.md` — a path-scoped denial must log which
  grant/path/verb was refused (secret-free), like the existing `capability_denied` error

## Key Files

- `packages/qfs/crates/runtime/src/caps.rs` - `CapabilitySet` (`grants: HashSet<(DriverId, String)>`, `grant()`, `allows(target, kind)`) — the type to make path-aware, additively
- `packages/qfs/crates/runtime/src/interpreter.rs` - the preview/commit re-check that calls `caps.allows(...)` per effect (lines ~127, ~210)
- `packages/qfs/crates/runtime/src/error.rs` - `EffectError::CapabilityDenied` (extend the message to name the path scope)
- `packages/qfs/crates/qfs/src/commit.rs` - one-shot uses `allow_all()`; the server path is where a policy-derived `CapabilitySet` is built
- `packages/qfs/crates/server/` - `CREATE POLICY` → handler `CapabilitySet` construction (where a path-scoped grant must be emitted/parsed)
- `packages/qfs/crates/parser/src/ast.rs` - `DdlKind::Policy` / the `CREATE POLICY` grammar, if a path-scope clause is added to the policy surface

## Related History

- [20260704001233-implement-sqlite-dbms-management.md](.workaholic/tickets/todo/a-qmu-jp/20260704001233-implement-sqlite-dbms-management.md) - The SQLite DBMS ticket that surfaced this gap; its Implementation Progress section records the finding
- [docs/adr/0009-sql-provisioning-and-ddl-semantics.md](docs/adr/0009-sql-provisioning-and-ddl-semantics.md) - §6 authorization model + the deny/allow matrix this ticket makes enforceable
- [docs/adr/0008-multi-host-account-model.md](docs/adr/0008-multi-host-account-model.md) - the host/operator (RBAC) model the PBAC layer composes with

## Implementation Steps

1. Model an optional **path scope** on a grant (e.g. `grants: HashSet<(DriverId, String, Option<PathScope>)>` or a small `Grant` struct). An unscoped grant matches any path (backward compatible); a scoped grant matches by path prefix / glob.
2. Make `CapabilitySet::allows(target, kind)` consult `target.path` against the grant's scope. Keep `allow_all` and unscoped grants behaving exactly as today (regression-guard existing runtime tests).
3. Extend `EffectError::CapabilityDenied` to carry the offending path so a denial is diagnosable (secret-free).
4. Decide and implement how a `CREATE POLICY` expresses a path-scoped grant (grammar/desugar in parser/core, construction in server) — the ADR §6 matrix: read / DML / DDL / provision as path-level grants over `/sql/<conn>` vs `/sql/<conn>/<table>`.
5. Wire the server's handler `CapabilitySet` construction to emit the path-scoped grants from a policy.
6. Prove the ADR §6 matrix end-to-end: a data-only policy admits `INSERT INTO /sql/<conn>/<table>` but denies `INSERT INTO /sql/<conn>` (create table); a read-only policy denies both; composition with the irreversible gate is independent (a DROP is refused by policy even with `--commit-irreversible`).

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `CapabilitySet` distinguishes a DDL effect (`INSERT`/`REMOVE` on `/sql/<conn>`) from a DML effect (same verb on `/sql/<conn>/<table>`): a path-scoped grant admits one and denies the other.
- Existing unscoped grants and `allow_all()` behave exactly as before (no regression in runtime/interpreter tests, or in the one-shot allow-all CLI path).
- A denial names the path in a secret-free `capability_denied` error.
- The ADR 0009 §6 deny/allow matrix holds end-to-end for at least the data-only and read-only rows, proven by hermetic tests; policy denial and the missing `--commit-irreversible` flag are never conflated.

**Verification method:**

- New hermetic unit tests on `CapabilitySet::allows` (path-scoped vs unscoped vs allow-all).
- Server-level tests building a `CapabilitySet` from a `CREATE POLICY` and asserting the matrix over `/sql/<conn>` vs `/sql/<conn>/<table>`.
- `cargo test --workspace`, `clippy --workspace --all-targets -- -D warnings`, `fmt --all --check`, `gen-docs --check`, `gen-skills --check`.

**Gate:**

- The full check set green, existing runtime security tests unbroken, and the §6 matrix demonstrated. This is a security-sensitive change — the reviewer confirms the additive path-scope cannot widen an existing grant.

## Considerations

- Security-critical: the change must be **additive** — an unscoped grant must keep matching any path, or every existing policy silently narrows (`crates/runtime/src/caps.rs`)
- One-shot CLI is `allow_all()`; do not accidentally start enforcing policies on the CLI path unless intended (`crates/qfs/src/commit.rs`)
- Keep the mechanism proportionate (access-control policy): path prefix/glob scope, not a general-purpose rules engine
- The catalog node's read/write asymmetry (SHOW TABLES reads `{name,kind}`; CREATE writes `{name,columns}`) means a "read-only" grant over `/sql/<conn>` still allows SHOW TABLES — decide whether that is intended (listing tables is a read) or whether provisioning/DDL need a distinct sub-path
