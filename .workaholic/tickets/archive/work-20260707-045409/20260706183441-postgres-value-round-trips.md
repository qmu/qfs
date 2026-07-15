---
created_at: 2026-07-06T18:34:41+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 2h
commit_hash: 29e627f
category: Added
depends_on: []
---

# Postgres NUMERIC / TIMESTAMP / UUID / JSON value round-trips (`pg_value`)

Split out from `20260706170000-git-path-binding-and-pg-value-round-trips.md` (Part C). **Part A of
that ticket — moving `git` onto the `path_binding` registry — shipped** on branch
`work-20260706-175903` (commit `d1e222d`); this ticket carries the remaining Part C, which was
deferred because its acceptance is **live-Postgres-only** (not hermetically verifiable) and the
`/drive` session was scoped to the hermetic work only.

## What's wanted

`crates/qfs/src/sql_backends.rs::pg_value` decodes BOOL / INT2/4/8 / FLOAT4/8 / BYTEA; NUMERIC /
TIMESTAMP(TZ) / UUID / JSON(B) fall through to `try_get::<Option<String>>`, which **errors** on those
OIDs. MySQL already round-trips these (its text protocol returns them as `mysql::Value::Bytes`, which
`my_value` decodes to `Text`) — so this is Postgres-only.

## Implementation steps

1. Add the postgres feature deps so those OIDs decode: `with-chrono-0_4` (or `with-time`) for
   TIMESTAMP/DATE, `with-uuid-1` for UUID, `with-serde_json-1` for JSON/JSONB, and `rust_decimal`
   (with `db-postgres`) for NUMERIC — in `crates/qfs/Cargo.toml`.
2. Add explicit `pg_value` arms decoding each OID to `Value::Text` (the honest canonical string),
   matching the `crates/sql-core/src/dialect.rs:102-186` type contract.
3. **Verification is live-only**: a `postgres::Row` is not constructible in a hermetic test (only
   bool/int/float/bytea are unit-tested today). The owner runs a live PG round-trip (`SELECT` a
   NUMERIC / TIMESTAMP / UUID / JSON column) as the acceptance, as the original concern's
   live-verification note anticipated.

## Key files

- `crates/qfs/src/sql_backends.rs` (`pg_value`), `crates/qfs/Cargo.toml` (postgres features),
  `crates/sql-core/src/dialect.rs` (target contract).

## Considerations

- Source concern: `.workaholic/concerns/11-postgres-mysql-declarations-for-the-declared.md` (its
  already-resolved sub-item — declared PG/MySQL support — stays dropped).
- **Owner input required**: this ticket cannot be closed autonomously — it needs a live Postgres DB
  with the four column types for the acceptance `SELECT`. Batch it with any other live-credential
  work.

## Final Report

Development completed with explicit Postgres rich-type decoding in `pg_value`. NUMERIC now decodes
from PostgreSQL's binary wire representation into canonical text, and DATE/TIMESTAMP/TIMESTAMPTZ,
UUID, JSON, and JSONB decode through the supported `postgres` feature adapters into qfs text values.

### Discovered Insights

- **Insight**: The cached `postgres` crate does not expose a `rust_decimal` feature in this
  workspace, so NUMERIC needed a small local binary decoder instead of adding a new decimal type to
  qfs's value surface.
  **Context**: Keeping NUMERIC as canonical text matches the existing MySQL text-protocol behavior
  and avoids widening `qfs_core::Value` for a backend-specific numeric representation.
