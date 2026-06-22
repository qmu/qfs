//! `cfs-server` — the server face of cfs (RFD-0001 §8).
//!
//! `cfs serve <config.cfs>` boots a long-lived server whose endpoints, triggers,
//! jobs, views, policies, and webhooks are **data** managed with cfs — because the
//! **server is a driver** over `/server/...` (see [`mount`] and fidelity guard G4).
//!
//! E0 ships [`serve`] as a stub returning a structured [`CfsError::NotImplemented`]
//! and the [`mount`] module reserving the server-as-a-driver seam. No HTTP, no
//! bindings (`ENDPOINT`/`TRIGGER`/`JOB`/`VIEW`/`WEBHOOK`/`POLICY`) — those land in E7.
//!
//! ## wasm-friendliness (boundary guard B7)
//! E0 introduces no sockets/threads here; the real server impl is gated behind E7
//! and the Cloudflare Workers deployment mapping (RFD §8).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod mount;

use std::path::Path as FsPath;

use cfs_core::CfsError;

/// Boot the server from a `.cfs` config file (RFD-0001 §8).
///
/// E0 stub: returns [`CfsError::NotImplemented`] — proving the `cfs serve` dispatch
/// seam from `cfs-cmd` reaches a structured error, not a panic.
///
/// # Errors
/// Always returns [`CfsError::NotImplemented`] at E0.
pub fn serve(_config: &FsPath) -> Result<(), CfsError> {
    Err(CfsError::NotImplemented { feature: "serve" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_returns_not_implemented() {
        let err = serve(FsPath::new("x.cfs")).unwrap_err();
        assert!(matches!(err, CfsError::NotImplemented { feature: "serve" }));
        assert_eq!(err.code(), "not_implemented");
    }

    #[test]
    fn server_mount_seam_is_reserved() {
        assert_eq!(mount::SERVER_MOUNT, "/server");
    }
}
