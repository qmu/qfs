-- §13 self-hosting integrations (blueprint §13): the DECLARED-DRIVER registry.
--
-- One row per declaration a `CREATE DRIVER`/`CREATE TYPE`/declared `CREATE VIEW`/`CREATE MAP`
-- script desugars to (`INSERT INTO /sys/drivers`), tagged by `kind`. The evaluator (next ticket)
-- reads these rows to build a live wire mount when the declared driver is connected.
--
-- Declaration TEXT + selectors ONLY. There is structurally NO column a secret value could ride in
-- (the credential-free-script contract, §13): `auth` is a SCHEME descriptor (bearer/header name/
-- oauth2 URLs), never a token; the token lives in the account layer (§8).
CREATE TABLE IF NOT EXISTS sys_drivers (
    id           INTEGER PRIMARY KEY,
    -- driver | type | view | map
    kind         TEXT NOT NULL,
    -- the driver name (kind=driver) or the node path (kind=type/view/map)
    name         TEXT NOT NULL,
    -- kind=driver: the wire base URL (`AT '<url>'`)
    base_url     TEXT,
    -- kind=driver: the auth descriptor JSON (a scheme, never a token)
    auth         TEXT,
    -- kind=driver: the pagination descriptor JSON (cursor / link)
    pagination   TEXT,
    -- kind=view: the declared `OF <type-path>` contract
    of_type      TEXT,
    -- kind=map: the mapped verb (INSERT/UPSERT/UPDATE/REMOVE) or `CALL <driver>.<action>`
    verb         TEXT,
    -- kind=type: the columns JSON; kind=view/map: the body statement as serde JSON
    body         TEXT,
    -- kind=map: the declared IRREVERSIBLE flag
    irreversible INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
