-- System DB — migration #17 (ticket 20260716143641 — re-home the declarative tables):
-- `path_binding` + `connection_consent` move HOME to the System DB, so a config write lands in the
-- SAME single-DB transaction as its t76 audit row and its `sys_ddl_events` entry (the
-- `insert_driver` pattern) — making blueprint §16's "every applied effect lands in the WORM tail"
-- true for CONNECT/DISCONNECT and account declare/remove, which previously emitted only a
-- best-effort post-commit AuditEvent from the Project DB (a cross-DB write cannot share one
-- transaction; SQLite WAL gives ATTACH no cross-file atomicity).
--
-- APPEND-ONLY: migrations #1..#16 are FROZEN (the checksum guard forbids editing a shipped
-- migration); these tables ship as a NEW version. The Project-DB originals (migrations #3 and #8,
-- plus the #9/#12 ALTER history folded into these fresh CREATEs) go DEAD but NOT dropped in this
-- release — a one-shot boot copy (`qfs::store`) moves the rows first, and the drop is a LATER
-- Project-DB migration once a release containing the copy has shipped (data-safety sequencing:
-- the drop must never be able to run before the copy has).
--
-- The boundary this re-draws: the Project DB is the VAULT proper (secret_store, key slots,
-- rotation, E2E) — one file holds secret material; this file holds everything declarative, plus
-- the ledger that observes it.
--
-- SELECTORS + METADATA ONLY — never a secret (both headers below restate their originals' rule).

-- The DEFINED-PATH binding registry (CONNECT/DISCONNECT — the `CONNECT` model, t100020; the
-- mount-coordinate columns of ADR 0008 and the multi-app column of 20260706175249 are folded in).
-- `secret_ref` is a REFERENCE (`env:VAR` / `vault:driver/connection`) resolved at USE time, never
-- a secret VALUE; `account` is a LABEL, never a token.
CREATE TABLE IF NOT EXISTS path_binding (
    path       TEXT PRIMARY KEY,
    driver_id  TEXT,
    at_locator TEXT,
    secret_ref TEXT,
    alias_of   TEXT REFERENCES path_binding(path) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    host       TEXT NOT NULL DEFAULT 'local',
    account    TEXT,
    app        TEXT
);

-- The cloud-connection CONSENT ledger (t54 / roadmap M4; the `app` column of 20260706175249
-- folded in). One row per `(driver, connection)` records that a signed-in operator GRANTED that
-- cloud connection explicit consent — the load-bearing state the commit-time bind gate consults.
-- The refresh token itself stays ENCRYPTED in the Project DB's `secret_store`; this table records
-- only that consent happened, by whom, for which scope, and when.
CREATE TABLE IF NOT EXISTS connection_consent (
    driver     TEXT NOT NULL,
    connection TEXT NOT NULL,
    subject    TEXT NOT NULL,
    scope      TEXT NOT NULL DEFAULT '',
    granted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    app        TEXT,
    PRIMARY KEY (driver, connection)
);
