---
created_at: 2026-07-04T00:12:32+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort: 2h
commit_hash: 5a37f52
category: Added
depends_on:
---

# Design ADR: SQLite database provisioning and DDL semantics (manage a SQLite db as if MySQL)

## Overview

Write the keystone design ADR that lets qfs **create and manage a SQLite database as if it were a
MySQL server** — get into a console, create a database, create tables, list what exists, and drop
things — entirely through the one pipe-SQL language. This fills the gap t17 explicitly left open
("Schema DDL — not in this ticket") and goes beyond CREATE CONNECTION, which only *references* an
existing database and never provisions one.

The ADR follows the precedent of the CREATE CONNECTION grammar ADR
(20260630004110-design-connection-declaration-grammar.md): decide the semantics on paper first,
with every proposed statement conforming to the closed-core grammar contract (zero new frozen
keywords, RFD-0001 §3), then hand the decisions to the implementation ticket
(20260704001233-implement-sqlite-dbms-management.md, which depends on this one).

Working direction recorded at ticket time (developer was away when the scoping questions were
asked; the recommended defaults were taken and the ADR must confirm or overturn them):

- **Statement surface**: statement-level `CREATE DATABASE` via contextual idents (the CREATE
  CONNECTION / t31 pattern), desugaring to effect plans — matching the owner's standing preference
  that setup is "part of syntax, not a subcommand" (see the CREATE ACCOUNT ticket).
- **Console**: extend the existing FTP-like shell (cd into /sql/<conn>, operate there) rather than
  adding a mysql-style sub-mode — the modeless-design policy resists new modes.

Decision points the ADR must settle (each with a recorded rationale):

1. **Surface mechanics** — contextual-ident `CREATE DATABASE` / `CREATE TABLE` statements
   desugaring to effect plans (t31 server-DDL pattern) vs `CALL` procedures
   (`sql.create_database`, riding the just-shipped terminal-CALL → Call-effect lowering) vs a
   hybrid (statements as sugar over CALL procs). Zero new frozen keywords in any variant.
2. **Provisioning semantics** — what "create a database" *means* for SQLite: SqliteBackend::open()
   already creates the file if absent, so is CREATE DATABASE = provision file + declare connection
   in one step? How does it relate to (or subsume) CREATE CONNECTION? What does it mean later for
   real MySQL/Postgres backends (an actual `CREATE DATABASE` DDL against the server)?
3. **Addressing** — SqlPath is Root | Connection | Table with no database level; how is a
   newly created database addressed, listed, and described? Does /sql/<conn> gain child databases
   for dialects that have them (MySQL "database" = the connection's schema today)?
4. **Catalog freshness** — ConnHandle introspects the Catalog ONCE and caches it; define the
   re-introspection rule after any DDL commit so DESCRIBE never lies.
5. **Irreversibility classification** — DROP DATABASE / DROP TABLE are irreversible and must be
   gated behind --commit-irreversible; decide the classification for CREATE / ALTER too.
6. **Console mapping** — which shell builtins (ls ≈ show tables, cd ≈ use) cover the MySQL-like
   experience, given every builtin must desugar to the same closed-core statements and stay
   equally reachable via one-shot `qfs run`.
7. **Authorization model** — who or what may provision, alter, and drop. Decide how DDL and
   provisioning effects are authorized, choosing the mechanism proportionate to the actual rules
   (access-control policy): the existing CREATE POLICY binding (DdlKind::Policy) as the gate over
   DDL-class effects, per-procedure `requires_scopes` on ProcSigs (the github-driver vocabulary)
   if a CALL surface is chosen, and per-connection capability declarations so a connection can be
   opened read-only or data-only (DML yes, DDL no). Classify database management as an
   administrative surface distinct from data reads/writes (admin-isolation lens), and state how
   the authorization layer composes with — not replaces — the irreversible-commit gate
   (defense-in-depth: independent controls, either alone refusing a destructive DDL).
8. **Account model for non-local use** — when qfs runs as a server (endpoints, jobs, triggers)
   rather than a single-owner local CLI, define who the authorization subjects are: multiple
   user accounts on one host, groups, and how roles and policies combine — an RBAC × PBAC
   composition where roles grant coarse operation classes (read / DML / DDL / provision / drop)
   and policies (CREATE POLICY) refine them per resource. Define the resource integration: grants
   attach to the same path tree the language addresses (/sql/<conn>, a database, a table), so the
   account model and the resource model are one vocabulary rather than a parallel ACL namespace.
   Align with the parked CREATE ACCOUNT ticket (accounts declared in-language, secret values
   out-of-band) and evaluate procuring identity (OS users, OIDC, existing auth solutions) before
   inventing one (auth-procurement policy). The local single-owner CLI remains the degenerate
   case: one implicit account with every role.

## Policies

The standard engineering policies that govern this ticket. The implementing session MUST read each
linked policy hard copy before writing the ADR and keep every decision defensible against that
policy's Goal, Responsibility, and Practices.

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:planning` / `policies/modeling-centric-design.md` — the ADR is requirements analysis through a model: databases, connections, catalogs, and effects must be modeled before code
- `workaholic:planning` / `policies/terminology.md` — "database" vs "connection" vs "catalog" must land in the project dictionary with one word per concept before the grammar names them
- `workaholic:implementation` / `policies/persistence.md` — schema-first: the managed-database semantics treat explicit schemas as first-class, describable state
- `workaholic:design` / `policies/modeless-design.md` — the console experience must not become a mode; every operation stays reachable one-shot
- `workaholic:design` / `policies/vendor-neutrality.md` — "as if MySQL" is a uniform qfs-native surface, not a leak of MySQL client protocol or SQLite C-API shapes into the language
- `workaholic:design` / `policies/access-control.md` — choose the authorization mechanism proportionate to the actual rules; start simple (connection capabilities, policy gate) and escalate only if a simpler model cannot express them
- `workaholic:design` / `policies/defense-in-depth.md` — authorization and the irreversible-commit gate are independent layers; the breach or misconfiguration of one must not expose destructive DDL
- `workaholic:design` / `policies/admin-isolation.md` — database management is an administrative surface; keep it distinguishable from ordinary data access so a data-level credential does not imply DDL rights
- `workaholic:design` / `policies/auth-procurement.md` — evaluate existing identity/auth solutions (OS users, OIDC, established libraries) before building a custom account system; the security surface of custom identity is wide
- `workaholic:design` / `policies/per-tenant-database.md` — multiple users on one host raises the isolation question early; decide the isolation model before data accumulates rather than retrofitting it
- `workaholic:implementation` / `policies/objective-documentation.md` — the ADR states what the binary will verifiably do; generated docs and cookbook recipes stay non-aspirational

## Key Files

- `packages/qfs/crates/parser/src/ast.rs` - DdlKind (server bindings + Connection only) and the keyword-freeze doc-comments; the constraint every proposed statement must satisfy
- `packages/qfs/crates/core/src/ddl/connections.rs` - CREATE CONNECTION parsing (contextual idents, DRIVER/AT/SECRET); the grammar baseline CREATE DATABASE must stay consistent with
- `packages/qfs/crates/plan/src/node.rs` - EffectKind (incl. Call(ProcId)) and the irreversible flag; where a DDL effect lands in the plan model
- `packages/qfs/crates/core/src/eval.rs` - terminal-CALL → Call-effect lowering (commits 5faab4a, 0c4aac8); the mechanism a CALL-based surface would ride
- `packages/qfs/crates/driver-sql/src/path.rs` - SqlPath = Root | Connection | Table; no database level exists — the addressing decision starts here
- `packages/qfs/crates/driver-sql/src/conn.rs` - SqlBackend trait + ConnHandle catalog-cached-once; commit_transaction takes only DmlOp (no DDL slot)
- `packages/qfs/crates/qfs/src/sql.rs` - SqliteBackend::open() "creating if absent" — today's only (implicit) database-creation path
- `packages/qfs/crates/qfs/src/sql_backends.rs` - MySQL backend scopes introspection to one database (schema = db name); the cross-dialect meaning of "database"
- `packages/qfs/crates/exec/src/shell/session.rs` - shell builtins desugar to closed-core statements; the console-mapping decision lands here
- `packages/qfs/crates/driver-github/src/effect.rs` - ProcSig with requires_scopes and irreversible: the existing per-procedure authorization vocabulary the DDL surface can reuse
- `packages/qfs/crates/parser/src/ast.rs` (DdlKind::Policy) - the existing CREATE POLICY binding; candidate gate for DDL-class effects in the authorization decision
- `.workaholic/RFDs/0001-qfs-architecture.md` - §3 keyword freeze, §5 archetypes/verbs, §6 preview/commit — the contract the ADR must cite

## Related History

The SQL surface exists but stops at tables: t17 shipped the /sql driver with DDL explicitly out of
scope, CREATE CONNECTION only points at existing databases, and the shipped shell is FTP-shaped —
so provisioning semantics are genuinely new, with strong precedents for how CREATE forms desugar.

Past tickets that touched similar areas:

- [20260622214650-t17-driver-sql-databases.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t17-driver-sql-databases.md) - Shipped the /sql/<conn>/<table> driver; explicitly excluded schema DDL (the gap this ADR fills)
- [20260630004110-design-connection-declaration-grammar.md](.workaholic/tickets/archive/work-20260629-110121/20260630004110-design-connection-declaration-grammar.md) - The CREATE CONNECTION grammar ADR; the direct precedent in shape and constraints
- [20260622214650-t31-server-binding-ddl.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t31-server-binding-ddl.md) - CREATE forms as pure sugar desugaring to effect plans; the established CREATE-statement pattern
- [20260622214650-t28-cli-interactive-shell.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t28-cli-interactive-shell.md) - The FTP-like shell ("get into a console" precedent); adds no new execution semantics
- [20260630203060-postgres-mysql-backends-podman-compose.md](.workaholic/tickets/archive/work-20260629-110121/20260630203060-postgres-mysql-backends-podman-compose.md) - Live Postgres/MySQL backends; the cross-dialect reality CREATE DATABASE must map onto
- [20260703040000-create-account-language-surface.md](.workaholic/tickets/todo/a-qmu-jp/20260703040000-create-account-language-surface.md) - The "setup is syntax, not a subcommand" preference and additive-contextual-ident rule this ADR reuses

## Implementation Steps

1. Read RFD-0001 (§3 keyword freeze, §5 archetypes, §6 effects) and the CREATE CONNECTION ADR's
   output; locate where that ADR's decision record landed and follow the same convention for this
   one.
2. Draft the ADR covering all eight decision points in the Overview, with at least the
   statement-surface question argued as a genuine comparison (statements vs CALL vs hybrid),
   citing the t31 desugar pattern and the new Call-effect lowering as evidence.
3. For each decided statement form, write the exact example statements the cookbook will later
   teach, and check each against the grammar constraints (contextual idents only; no new frozen
   keywords; unsupported verbs rejected at parse time).
4. Define the cross-dialect mapping table: what CREATE DATABASE / list databases / USE mean for
   sqlite (file), mysql (real DDL + schema scoping), postgres (database/schema split) — even
   though only SQLite ships in the implementation ticket.
5. Define the catalog-refresh rule after DDL commit and the irreversibility classification for
   every new operation.
6. Record the console mapping (which builtins, what they desugar to) under the modeless
   constraint.
7. Decide the authorization model (decision point 7): survey the existing CREATE POLICY binding,
   ProcSig requires_scopes, and connection declaration surface; choose the proportionate
   mechanism, define how it composes with the irreversible gate, and write the deny/allow matrix
   (subject × operation class: read / DML / DDL / provision / drop) the implementation ticket
   will test against.
8. Decide the non-local account model (decision point 8): survey the parked CREATE ACCOUNT
   ticket, the server binding surface (endpoints/jobs/triggers run as *someone*), and existing
   identity options per the auth-procurement policy; define accounts, groups, the RBAC × PBAC
   composition, and how grants attach to the addressed resource tree — and state explicitly that
   the local CLI is the one-implicit-account degenerate case. The matrix subject axis from step 7
   uses this model.
9. Hand the decisions to 20260704001233-implement-sqlite-dbms-management.md (update that ticket's
   assumptions if the ADR overturns a recorded default).

## Quality Gate

Captured at ticket time; the developer was away during interrogation, so the recommended defaults
below were recorded — `/drive` surfaces this gate at approval and the developer may tighten it
there.

**Acceptance criteria** — the checkable conditions that must hold:

- The ADR document exists (in the same location/convention as the CREATE CONNECTION ADR's output)
  and records a decision plus rationale for all eight numbered decision points in the Overview,
  including a concrete deny/allow matrix for the authorization model whose subject axis
  (accounts / groups / roles / policies) comes from the decided account model.
- Every example statement in the ADR either parses against the current binary, or is explicitly
  marked as a proposed additive form with the contextual-ident/no-new-frozen-keyword rule cited.
- The ADR introduces zero new frozen keywords and leaves the describe→preview→commit contract and
  the four-archetype model unchanged (RFD-0001 §3/§5/§6 conformance stated in the document).
- `cargo test --workspace` remains green (this ticket changes no product code).

**Verification method** — the commands/tests/probes that prove them:

- `cd packages/qfs && cargo test --workspace` (unchanged, green).
- Parse-check each ADR example marked as currently-valid with the existing binary/parse harness;
  proposed-form examples are checked instead against the grammar-constraint checklist in the ADR.
- Cross-read against RFD-0001 §3/§5/§6 and the CREATE CONNECTION ADR for consistency.

**Gate** — what must pass before approval:

- All eight decision points decided with rationale, examples conforming as above, workspace tests
  green, and the developer approves the ADR content at `/drive` (this is a design deliverable —
  the developer's content approval is the substantive gate).

## Considerations

- The keyword freeze forbids new grammar verbs; any statement form must use contextual idents and
  desugar onto existing effect machinery (`packages/qfs/crates/parser/src/ast.rs`)
- CREATE CONNECTION only references an existing database — decide explicitly whether CREATE
  DATABASE subsumes, composes with, or stays orthogonal to it
  (`packages/qfs/crates/core/src/ddl/connections.rs`)
- ConnHandle caches the Catalog once at construction; without a defined refresh rule, DESCRIBE
  lies after any DDL (`packages/qfs/crates/driver-sql/src/conn.rs`)
- MySQL's "database" is today modeled as the connection's schema scope — the ADR's addressing
  decision determines whether that model survives (`packages/qfs/crates/qfs/src/sql_backends.rs`)
- The in-flight v0.0.19 resume ticket wires terminal-CALL → Call-effect lowering; a CALL-based
  surface depends on that machinery landing first
  (`.workaholic/tickets/todo/a-qmu-jp/20260703200500-resume-0019-call-lowering-shipped-code.md`)
- The account model (decision point 8) is decided here but implemented later: the SQLite
  implementation ticket enforces authorization with the decided subject model in its local
  degenerate form, and full multi-user server accounts (accounts/groups/identity wiring) are
  expected to become their own ticket after the ADR — coordinate with the parked CREATE ACCOUNT
  surface (`.workaholic/tickets/todo/a-qmu-jp/20260703040000-create-account-language-surface.md`)
- qfs is experimental: hard breaks are correct; do not design migration/compat paths for the
  existing CONNECT surface if the ADR decides to reshape it

## Final Report

Development completed as planned. Delivered `docs/adr/0009-sql-provisioning-and-ddl-semantics.md`
(all eight decision points settled with rationale) plus its sidebar entry in
`docs/.vitepress/config.mts`. No product code changed, so the workspace tests are unaffected.

The core decision **overturned the recorded working default**: rather than a statement-level
`CREATE DATABASE` (contextual-ident sugar), the ADR lands on RFD-0001 §3's own answer — "CRUD is
universal; the path is the type; no per-driver create verb." Creating a table is `INSERT INTO
/sql/<conn>`, provisioning a database is `INSERT INTO /sql`, dropping is `REMOVE` behind the
irreversible gate, and only genuinely irreducible admin ops (VACUUM/ANALYZE) are `CALL`
procedures. This keeps the grammar frozen (zero new keywords) and makes `DESCRIBE` teach DDL for
free — a stronger fit than the contextual-ident `CREATE DATABASE` the ticket had proposed.

### Discovered Insights

- **Insight**: `docs/adr/0008-multi-host-account-model.md` already exists and decides the account
  model in depth (local is an implicit host; the mount carries `(host, driver, account)`; the
  `qfs init`/`host`/`app`/`account`/`connect` verb split; `CREATE POLICY` for least privilege).
  **Context**: The ticket's decision point 8 ("account model for non-local use") was largely
  already answered. ADR 0009 therefore defers to ADR 0008 for RBAC and layers only the SQLite DDL
  authorization (PBAC via `CREATE POLICY`, plus the local single-operator degenerate case) on top,
  rather than designing a parallel account scheme. A future reader scoping multi-user server
  accounts should start from ADR 0008, not reinvent it.
- **Insight**: The frozen server-DDL keywords (`CREATE ENDPOINT|TRIGGER|JOB|VIEW|WEBHOOK|POLICY`)
  are the *only* `CREATE …` forms in the closed core — `CREATE TABLE`/`CREATE DATABASE` are
  deliberately absent. **Context**: This is why the ADR routes DDL through `INSERT INTO` catalog
  nodes instead of new `CREATE` statements; the server bindings desugar to `INSERT INTO /server/…`
  precisely because the keyword set cannot grow, and the SQL catalog surface follows the identical
  pattern.
