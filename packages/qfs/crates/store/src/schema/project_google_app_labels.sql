-- Project DB — migration #12 (20260706175249): multiple OAuth apps per provider.
--
-- APPEND-ONLY: migrations #1..#11 are FROZEN. This forwards the existing mount/account
-- coordinate model with one more selector: which provider app minted or should service the
-- account. Selectors + metadata only — never a token or client secret.
ALTER TABLE connection_consent ADD COLUMN app TEXT;
ALTER TABLE path_binding ADD COLUMN app TEXT;
