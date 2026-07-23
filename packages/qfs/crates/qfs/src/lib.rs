#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `qfs` library facet — the composition-root code shared by the `qfs` binary (`main.rs`)
//! and the `xtask` build tooling.
//!
//! The `qfs` crate is primarily the **single binary** (blueprint §11). It also exposes this
//! thin library facet so two consumers can reuse the binary's *own* registries instead of
//! re-deriving them:
//!
//! - `main.rs` wires [`describe::describe_registry`], the serve launcher, the shell, etc.
//! - `xtask` (the build tool, never shipped) calls [`docs::gen_docs`], which renders the
//!   reference docs from [`catalog::driver_catalog`] — built by walking the **same**
//!   describe registry the binary ships. That is the anti-drift guarantee (t40): the docs
//!   are generated from the binary's live introspection surface, never authored twice.
//!
//! The modules that wire the runtime-coupled serve/shell/host facets (`serve`, `shell`,
//! `host`, `job`, `watchtower`, `serve_builtins`) stay `pub(crate)`-only in spirit but are
//! exposed here as modules so `main.rs` can reference them through the crate root; nothing
//! outside this crate links the binary as a library except `xtask`, which touches only the
//! pure [`catalog`]/[`docs`]/[`version`] surface (no runtime, no creds, no I/O).

pub mod account;
pub mod agent;
pub mod apply_facets;
pub mod audit;
pub mod billing;
pub mod broker;
pub mod catalog;
pub mod cf;
pub mod claude;
pub mod clients;
pub mod cloud_mounts;
pub mod collection_mount;
pub mod commit;
pub mod connection;
pub mod connections_config;
pub mod console;
pub mod dashboard;
pub mod declared_driver;
pub mod declared_eval;
pub mod describe;
pub mod directory;
pub mod docs;
pub mod dump;
pub mod e2e_store;
pub mod fs;
pub mod git;
pub mod google;
pub mod host;
pub mod hosts;
pub mod identity;
pub mod init;
pub mod invite;
pub mod job;
pub mod mcp;
pub mod migration_guard;
pub mod mount_adapter;
pub mod oauth;
pub mod objstore;
pub mod path_binding;
pub mod provision;
pub mod read_facets;
pub mod restore;
pub mod secret_ref;
pub mod secret_store;
pub mod serve;
pub mod serve_builtins;
pub mod server_face;
pub mod session;
pub mod session_unlock;
pub mod shared_connection;
pub mod shell;
pub mod sql;
pub mod sql_backends;
pub mod sql_contracts;
pub mod store;
pub mod sweeper;
pub mod sys;
pub mod telemetry;
pub mod transform;
pub mod transform_providers;
pub mod transport;
pub mod tty;
pub mod tunnel;
pub mod type_catalog;
pub mod vault;
pub mod version;
pub mod view;
pub mod watchtower;
pub mod worm;

/// Test-only environment isolation (one crate-wide lock + a fresh per-test config home). See the
/// module for why every env-mutating test must route through it.
#[cfg(test)]
mod testenv;
