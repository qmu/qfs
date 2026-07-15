-- §15 transform predicates (blueprint §15, decision W): the TRANSFORM-DEFINITION registry.
--
-- One row per `CREATE TRANSFORM <name> INPUT (…) OUTPUT (…) PROVIDER … MODEL … [EFFORT …]
-- [SECRET '<ref>']` declaration (its desugar target is `INSERT INTO /transform`), and the target
-- of `REMOVE TRANSFORM <name>` (`REMOVE /transform/<name>`). The evaluator (the plan spine + the
-- executor, the sibling tickets) reads these rows to resolve a `|> transform <name>` pipe stage.
--
-- A definition is DATA (declare → store → activate), so it is a relational row like the declared
-- drivers (`sys_drivers`, v14) — not a bespoke config file.
--
-- Definition TEXT + selectors ONLY. There is structurally NO column a secret VALUE could ride in
-- (the credential-free-definition contract, mirroring §13): `secret_ref` is a REFERENCE
-- (`env:<VAR>` / `vault:<path>`) resolved lazily at COMMIT, never a token, never resolved at
-- DESCRIBE. The cardinality MODE (row-wise / relation-wise / extraction) is NOT a column — it is
-- DERIVED from `input` on every read (a total function, `qfs_types::derive_mode`), so a stored flag
-- can never drift from the declared shape.
CREATE TABLE IF NOT EXISTS sys_transforms (
    id          INTEGER PRIMARY KEY,
    -- the transform definition name — the `<name>` in `CREATE TRANSFORM <name>` and the
    -- `/transform/<name>` path segment. Unique: a name identifies exactly one definition.
    name        TEXT NOT NULL UNIQUE,
    -- the declared INPUT schema as the flat column-descriptor JSON array the CREATE TRANSFORM
    -- grammar emits (`[{"name","type","nullable"}]`, `type` = the canonical `ColumnType::parse`
    -- string) — the shape `TransformDef::from_stored` decodes
    input       TEXT NOT NULL,
    -- the declared OUTPUT schema, same column-descriptor JSON shape as `input`
    output      TEXT NOT NULL,
    -- the model provider (e.g. an id the binary's provider seam resolves); a selector, never a token
    provider    TEXT NOT NULL,
    -- the model name/id the provider is asked for
    model       TEXT NOT NULL,
    -- the optional effort/budget hint (NULL when unspecified)
    effort      TEXT,
    -- the SECRET REFERENCE (`env:<VAR>` / `vault:<path>`), NEVER an inline value; NULL when the
    -- provider needs no per-definition secret. Resolved lazily at COMMIT, never at DESCRIBE.
    secret_ref  TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
