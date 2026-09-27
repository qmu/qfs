#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `qfs-cluster` — the first slice of the qfs cluster.
//!
//! A qfs started as a cluster **host** accepts **members** over a persistent WebSocket. Each
//! member authenticates with a join token the host minted ([`token`]), then sends a heartbeat
//! ([`frame::Report`]) carrying its hostname, CPU, memory, disk and Claude Code sessions. The host
//! keeps the aggregate in an in-memory [`registry::Registry`] (members are stateless; the host is
//! the only state) and answers a loopback-only JSON read API on the same listener ([`host`]).
//! A member may also **borrow** a host mount ([`borrow`]): it sends a statement, the host checks
//! its grant table ([`grants`]) and runs it, and only the result crosses the wire.
//!
//! The trust model is `docs/adr/0008-cluster-trust-model.md`. This crate is a LEAF consumed only
//! by the terminal `qfs` binary, so its tokio dead-ends there.

pub mod accounts;
pub mod borrow;
pub mod frame;
pub mod grants;
pub mod host;
pub mod member;
pub mod registry;
pub mod sample;
pub mod token;

pub use accounts::{AccountRow, Accounts};
pub use borrow::{Borrow, Execute, PathsOf, Touched};
pub use frame::{Frame, Report, SessionReport};
pub use grants::Grants;
pub use host::{fetch_json, serve, serve_with, DEFAULT_LISTEN};
pub use member::{run_member, run_once, MemberConfig, MemberError};
pub use registry::{MemberRow, Registry, SessionRow};
pub use token::{mint, verify, TokenClaims, TokenError};

/// Seconds since the Unix epoch (0 if the clock is before it — never a panic).
#[must_use]
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
