---
created_at: 2026-07-01T10:00:20+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Added
depends_on: [20260701100010-design-defined-path-model-grammar.md]
---

# Defined-path declaration: grammar + config + persisted binding

Part of EPIC `20260701100000`. Implements the declaration surface decided in the design keystone
(`100010`). **Subsumes** the abandoned `CREATE ALIAS` ticket (`20260630204000`) — the path-binding
grammar is built here ONCE as "defined path", not as a throwaway alias shorthand.

## Plan (final syntax per the keystone decision)

1. **Parser AST + grammar (contextual idents, freeze-safe).** Add the defined-path clause to
   `crates/parser/src/{ast.rs,grammar.rs}`, mirroring the `CREATE CONNECTION` clauses
   (`conn_driver_clause`/`conn_at_clause`/`conn_secret_clause`, `grammar.rs:~1403`). The new
   path/mount clause MUST be a `word(...)` contextual ident — **no new frozen keyword** (`AT` is
   already taken). Parse the (possibly **recursive / multi-segment**) path token as a typed value
   object, not a bare string (`type-driven-design`).
2. **Config model + load.** Extend `DeclaredConnection` (`crates/core/src/ddl/connections.rs:17`) /
   add the binding to the `connections.qfs` model and `parse_connections`. The `connections_config`
   loader (`crates/qfs/src/connections_config.rs`) surfaces the declared `{path → driver + credential}`
   bindings to the registration redesign (`100040`).
3. **Persist the binding at `connection add`.** In `crates/qfs/src/connection.rs` (`run_connection`),
   when the user configures a credential, also persist the defined-path binding so a path resolves to
   `(driver id, connection, credential)`. Schema-first (a binding table in qfs's own DB), with FK
   integrity to the credential row and a defined deletion semantic (removing a connection removes its
   defined path). See `persistence` + `data-sovereignty`.
4. **Docs.** Regenerate `docs/{language,drivers}.md` via `xtask gen-docs`; `gen-docs --check` green.

## Key files

- `crates/parser/src/{ast.rs,grammar.rs,tests.rs}`, `crates/lang`,
  `crates/core/src/ddl/connections.rs`, `crates/qfs/src/{connections_config.rs,connection.rs}`,
  the binding-store migration in `crates/store` / the project DB schema.

## Considerations

- Stay freeze-safe: a parse-coverage / cookbook test must prove the new clause parses AND that the
  frozen keyword set is unchanged.
- The binding persistence reuses the t43 envelope-encrypted credential store seam; the binding row
  itself is selectors/metadata only (no secret), like the existing connection rows.
- Do NOT wire resolution/registration here — this ticket only DECLARES + STORES the binding;
  `100030` resolves it and `100040` registers it.

## Policies

- `implementation/type-driven-design` (typed path value object, additive grammar),
  `implementation/persistence` (binding schema, FK, deletion semantics),
  `design/data-sovereignty` + `design/defense-in-depth` (credential half of the binding),
  `planning/terminology` (defined path), `implementation/directory-structure`/`coding-standards`.
