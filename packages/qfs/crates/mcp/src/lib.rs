//! The **MCP serving binding** (t47, roadmap M2 — decisions C + K): qfs's JSON-RPC / Model Context
//! Protocol face, the third of "one engine, three faces" (CLI · HTTP · MCP).
//!
//! ## What it is
//! A client LLM (Claude) drives every service qfs fronts through FOUR tools that map 1:1 to qfs's
//! operating loop (roadmap §2.2): **`describe` → `preview` → `commit`**, plus **`connections`** for
//! discovery. This crate is:
//!   1. a PURE JSON-RPC 2.0 + MCP protocol core ([`jsonrpc`] + [`protocol`]): `initialize` /
//!      `tools/list` / `tools/call`, unit-testable against golden wire shapes with NO I/O; and
//!   2. the four tool handlers ([`tools`]) over an injected [`McpEngine`] — the live reads/plans/
//!      applies are supplied by the `qfs` binary, so this crate never touches a live driver; and
//!   3. an [`McpBinding`] ([`binding`]) — a pure, synchronous handler that frames `POST /mcp` into
//!      a JSON-RPC call, composed into the existing in-house HTTP listener by the binary (the
//!      MCP sibling of the t32 qfs-http / t33 qfs-cron / t34 qfs-watchtower serve leaves).
//!
//! ## Decision K — there is NO LLM in qfs
//! This crate has ZERO model/inference dependencies; it only EXPOSES tools. The tool descriptions
//! are prescriptive about WHEN to call each (describe-first, preview-before-commit) — that
//! prescriptiveness is what keeps a capable client model from guessing (roadmap §2.2). Text-to-SQL
//! is the client's job; qfs hosts no model.
//!
//! ## The safety floor is inherited verbatim
//! `describe` is pure (no creds/IO/network); `preview` applies zero effects; `commit` goes through
//! the SAME default-deny policy gate ([`qfs_server::gate_plan`]) and the SAME
//! [`qfs_core::IrreversibleGuard`] the CLI uses — no privileged shortcut, an irreversible plan
//! without ack is REFUSED; `connections` returns names/metadata only, redacted, never secrets.
//! Engine errors are surfaced secret-free (no token/path-secret/stack leak).
//!
//! ## Auth gap, stated honestly
//! The endpoint is UNAUTHENTICATED this milestone (bearer/OAuth is **t50**); it defaults to the
//! listener's localhost-only bind. The MCP *tool surface* exists, but a remote MCP connection /
//! "Claude can connect" does NOT work yet — that needs t48–t50. [`McpAuthorizer`] is the explicit
//! seam where t50 slots auth in front of the tools without touching a single handler.
//!
//! ## Topology (the leaf guard)
//! qfs-mcp depends on qfs-server (the policy gate) + qfs-exec (the plan/preview) — exactly the
//! role qfs-http/qfs-cron/qfs-watchtower play — and is consumed ONLY by the terminal `qfs` binary.
//! It does NOT depend on qfs-runtime: the apply is the injected [`McpEngine::apply`] closure, so
//! the COMMIT interpreter (and its tokio) stays in the binary. The
//! `mcp_binding_is_a_leaf_serve_consumer` guard pins this.

// Test modules assert/expect/unwrap freely; the strict workspace lint is relaxed under cfg(test).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod binding;
pub mod jsonrpc;
pub mod protocol;
pub mod tools;

pub use binding::{
    AllowLocalhost, AuthDecision, McpAuthorizer, McpBinding, MAX_MCP_BODY_BYTES, MCP_PATH,
};
pub use jsonrpc::{ErrorObject, Request, Response};
pub use protocol::{handle_request, initialize_result, tools_list_result, PROTOCOL_VERSION};
pub use tools::{
    call_tool, default_deny_policy, tool_descriptors, ConnectionInfo, EngineError, McpEngine,
    ToolDescriptor,
};

// Re-export the pure HTTP exchange DTOs (qfs-http-core) and the policy type (qfs-server) the
// binding + the `McpEngine` trait speak, so the `qfs` binary's composition root can adapt the
// listener onto them WITHOUT a direct dependency on either crate (its thin-entrypoint guard pins
// the binary off the lower spine; qfs-mcp is the leaf that legitimately binds both).
pub use qfs_http_core::{HttpMethod, HttpRequest, HttpResponse};
pub use qfs_server::Policy;
