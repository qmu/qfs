---
created_at: 2026-07-04T14:51:36+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash: bd54e87
category: Added
depends_on:
---

# Declared-driver surface: CREATE DRIVER / parameterized views / CREATE MAP, desugared to rows

## Overview

Implement the **declaration surface** of blueprint §13 (self-hosting integrations — approved
2026-07-04): the statements an integration script consists of, and their desugar into system-DB
rows. This ticket is grammar + desugar + storage only; evaluation is the next ticket
(`20260704145137`). Blueprint §13 is the authority for every surface decision.

- `CREATE DRIVER <name> AT '<base-url>' AUTH ( NONE | BEARER | HEADER '<name>' | OAUTH2
  (authorize '<url>' token '<url>' scopes '<…>') ) [PAGINATE …]` — contextual idents throughout
  (`DRIVER`/`AUTH`/`BEARER`/… follow the `AT`/`SECRET` clause precedent); **no clause can carry a
  secret value** (scripts are credential-free by construction — reject an inline token shape at
  parse time).
- **Parameterized definition nodes**: `{param}` template segments in a declared `CREATE VIEW`
  path, bound into the body pipeline (`CREATE VIEW /chatwork/rooms/{room}/messages AS
  /http/chatwork/rooms/{room}/messages |> DECODE json`), composing with `OF <type-path>` when
  the type surface lands.
- `CREATE MAP <verb|CALL <driver>.<action>> <node-path> AS <wire effect statement>
  [IRREVERSIBLE]` — the write/CALL mapping declaration.
- `PAGINATE ( CURSOR (next '<field>' param '<name>' MAX <n>) | LINK MAX <n> )` — the shipped
  `Pagination` enum, spoken; driver-level default with per-view override.
- **Desugar**: every declaration lowers to `INSERT INTO /sys/drivers` rows (the `/server`
  binding precedent — pure sugar, previewed, committed, audited). Row shape mirrors
  `RestApiConfig` + the view/map bodies as stored statement text (pre-parse-validated).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions
- `workaholic:implementation` / `policies/type-driven-design.md` — the declaration AST models auth/pagination as closed sums mirroring the shipped config enums
- `workaholic:implementation` / `policies/functional-programming.md` — declarations are data; desugar is pure
- `workaholic:design` / `policies/vendor-neutrality.md` — no manifest format; the language is the declaration format

## Key Files

- `packages/qfs/crates/parser/src/grammar.rs` - the definition-layer dispatch (`create_table_stmt` precedent for a contextual-noun statement; `conn_*_clause` precedent for AT/AUTH clauses)
- `packages/qfs/crates/parser/src/ast.rs` - declaration AST nodes (keyword freeze: contextual idents only)
- `packages/qfs/crates/driver-http/src/config.rs` - `RestApiConfig`/`AuthStrategy`/`Pagination` — the shapes the rows mirror
- `packages/qfs/crates/qfs/src/sys.rs` - the `/sys/*` write surface the desugar targets
- `docs/blueprint.md` §13 - the authority

## Implementation Steps

1. Grammar: `create_driver_stmt`, `{param}` segments in view paths (extend `path_expr` or a
   template variant), `create_map_stmt`, `PAGINATE`/`AUTH` clause parsers — all backtracking
   contextual-ident probes in the definition-layer dispatch.
2. Reject-at-parse: an inline secret-looking AUTH value (only HEADER *names* and OAuth *URLs*
   are strings; no token clause exists), malformed templates, a MAP body addressing a non-wire
   path.
3. Desugar to `/sys/drivers` effect rows; store view/map bodies as validated statement text.
4. Parser tests (the `create_table` test suite as the template) incl. a Chatwork-script
   round-trip fixture from blueprint §13's example; cookbook article + ratchet when the surface
   is teachable.

## Quality Gate

**Acceptance criteria:**

- The full §13 Chatwork example parses statement-for-statement and desugars to `/sys/drivers`
  rows (asserted in parser/e2e tests); zero new frozen keywords (`RESERVED_KEYWORDS` lock test
  unchanged).
- An AUTH clause cannot syntactically carry a token value; the negative test proves it.
- `CREATE VIEW` without `{param}` and existing definition statements are unaffected (dispatch
  regression tests, incl. the `CREATE VIEW`-is-server-DDL guard).

**Verification method:** `cargo test --workspace`; `clippy --workspace --all-targets -- -D
warnings`; `fmt --all --check`; `gen-docs --check`; `gen-skills --check`.

**Gate:** all green; the Chatwork fixture parses; keyword-freeze lock unchanged.

## Considerations

- `{param}` must not collide with glob segments or `@version` coordinates in `path_expr`
  (`packages/qfs/crates/parser/src/path`-handling)
- Stored bodies re-parse at evaluation time — store the text plus the parse-validated flag, not
  a serialized AST (hard-break freedom while the AST evolves)
- qfs is experimental: the `/sys/drivers` row shape may hard-break freely

## Progress (2026-07-05, work-20260705-032203)

Implemented surface + storage; all gates green.

- **Parser (in-parser desugar, the `CREATE TABLE`/`CONNECT` precedent).** `CREATE DRIVER`,
  `CREATE TYPE`, the PATH-named declared `CREATE VIEW`, and `CREATE MAP` desugar to
  `INSERT INTO /sys/drivers` (`Statement::Effect`) in `crates/parser/src/grammar.rs` — **no new
  `Statement` variant**, so the closed-core lock is untouched. All nouns are contextual idents; the
  `RESERVED_KEYWORDS` freeze test is unchanged (asserted by `declared_driver_nouns_add_no_frozen_keyword`).
  `AUTH` is credential-free by construction: `BEARER` takes no argument, so `AUTH BEARER '<tok>'` is a
  parse error (`driver_auth_bearer_carries_no_token_value`).
- **Storage.** New `SysNode::Drivers` (`/sys/drivers`) + its `sys_node_schema`/capabilities, a
  `sys_drivers` System-DB table (migration #14), the `insert_driver` backend write (transactional +
  self-auditing), and the applier route. `DESCRIBE`/scan and an install→scan round-trip pass.
- **AUTH/PAGINATE sums mirror the shipped `AuthStrategy`/`Pagination`** (`driver-http/config.rs`):
  `NONE|BEARER|HEADER '<name>'|OAUTH2(authorize/token/scopes)` and `CURSOR(next/param/MAX)|LINK MAX`,
  stored as JSON descriptors (no secret). OAUTH2 is a declaration superset (rides existing consent).

### Deviation from the ticket (recorded, defensible)

- **Bodies stored as serde AST JSON, not raw text.** The ticket suggests storing the pre-parse-validated
  *text*, but the parser has **no source-slice/text-capture mechanism** (it runs over tokens; `&str` is
  only at the `parse()` boundary), and the shipped precedent (`StatementSpec::canonical`) already stores
  deterministic AST JSON for deferred DDL bodies. So view/map bodies + type columns are stored as
  `serde_json` of the parsed node; the evaluator rehydrates via serde (no re-parse). qfs is experimental
  and `/sys/drivers` may hard-break, so the "text for hard-break freedom" rationale is moot here.

### §13 named park (feed to the blueprint)

- **Map-body codec shorthand.** Blueprint §13 writes the map body as
  `INSERT INTO /http/... VALUES (ENCODE json)`, but `ENCODE` is a pipe op, not a value expression, so
  that literal does not parse. Per this ticket's own Considerations ("a surface gap … not to hack
  around"), the core effect grammar was NOT polluted: the codec is the driver's `default_codec` (json),
  and the map body carries the wire target + verb. The full §13 example parses with this substitution
  (`full_chatwork_script_parses_statement_for_statement`). Blueprint §13 should either declare the map
  codec on the driver/map or add a codec clause — recorded for 145138's parks pass.
