//! Closed-core DDL (blueprint §3 frozen keywords). The server-binding DDL — the five frozen
//! `CREATE ENDPOINT|TRIGGER|JOB|VIEW|WEBHOOK` forms and their desugar to `INSERT INTO
//! /server/*` — lives here, in closed core, because the keywords are frozen and shared (not a
//! driver concern). See [`server`].

pub mod document;
pub mod server;
pub mod transform;
pub mod types;
