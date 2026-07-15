-- System DB — migration #15: replayable qfs DDL/config event log.
--
-- This is deliberately SEPARATE from the t76 `/sys/audit` stream. Audit remains metadata-only and
-- bounded locally; this table is the replay source for qfs configuration state. It stores normalized,
-- secret-free DDL/config events that can rebuild current-state tables such as `sys_drivers`,
-- `sys_policies`, and `sys_settings` without requiring a migration "down" model.
--
-- The payload column is JSON text owned by the qfs write path. It may contain selectors,
-- credential REFERENCES, auth SCHEMES, and declared body text, but never a plaintext token,
-- passphrase, ciphertext, or nonce. The schema gives plaintext secrets nowhere first-class to live.
CREATE TABLE IF NOT EXISTS sys_ddl_events (
    seq          INTEGER PRIMARY KEY,
    tx_id        TEXT NOT NULL,
    actor        TEXT NOT NULL,
    ts           TEXT NOT NULL,
    target_path  TEXT NOT NULL,
    verb         TEXT NOT NULL,
    source_text  TEXT,
    payload_json TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    prev_hash    TEXT NOT NULL,
    hash         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS sys_ddl_events_tx_id ON sys_ddl_events(tx_id);
CREATE INDEX IF NOT EXISTS sys_ddl_events_target_path_seq ON sys_ddl_events(target_path, seq);
