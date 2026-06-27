//! The `qfs serve` **MCP composition** (t47): the binary implements + injects the
//! [`qfs_mcp::McpEngine`] and composes the `POST /mcp` handler into the existing HTTP listener.
//!
//! Like the t32/t33/t34 serve leaves, `qfs-mcp` is a LEAF that consumes `qfs-server` (the policy
//! gate) and `qfs-exec` (the plan/preview) but NOT `qfs-runtime` — so the live half (the
//! describe registry, the real `build_plan`, the runtime-backed `apply`, the redacted connection
//! list) is injected HERE, where the binary already owns the runtime + the credential store. The
//! MCP protocol/tool logic stays pure in `qfs-mcp`; this module is only the wiring.
//!
//! ## The apply runs OFF the serve runtime (no nested `block_on`)
//! [`crate::commit::apply_plan`] builds its own current-thread tokio runtime and `block_on`s the
//! COMMIT interpreter. The MCP handler is invoked synchronously from inside the serve tokio
//! runtime (the listener's request future), and `block_on` panics if called on a runtime thread.
//! So [`ServeMcpEngine::apply`] offloads the commit to a dedicated OS thread (via
//! [`std::thread::scope`]) that owns no ambient runtime — the same "tokio dead-ends in the
//! terminal binary" discipline, just isolated from the listener's reactor.

use std::sync::Arc;

use qfs_core::Engine;
use qfs_mcp::{ConnectionInfo, EngineError, McpEngine};
use qfs_secrets::Secrets;

/// The binary's live [`McpEngine`]: holds the serve engine (mounts + codecs) the plan builder and
/// describe registry resolve against. The describe registry + connection store are built per-call
/// (cred-free / best-effort), exactly as the `qfs describe` and `qfs connection list` subcommands do.
pub struct ServeMcpEngine {
    engine: Arc<Engine>,
}

impl ServeMcpEngine {
    /// Build the engine over the shared serve [`Engine`].
    #[must_use]
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }
}

/// Map an executor error into the secret-free [`EngineError`] the MCP surface exposes. ExecError
/// is already secret-free (code + machine-facing message); we carry the stable `kind` as the code
/// and the message verbatim — no path-secret, token, or stack ever crosses this seam.
fn map_exec_err(e: &qfs_exec::ExecError) -> EngineError {
    EngineError::new(e.kind.as_str(), e.message.clone())
}

impl McpEngine for ServeMcpEngine {
    fn describe(&self, path: &str) -> Result<serde_json::Value, EngineError> {
        // The cred-free describe registry — the SAME one `qfs describe` consults (pure: no creds,
        // no I/O, no network; only the introspective driver half is ever touched).
        let registry = crate::describe::describe_registry();
        let (driver, _rest) = registry.resolve_path(path).ok_or_else(|| {
            EngineError::new(
                "unknown_mount",
                format!("no driver is mounted for `{path}` (describe registry)"),
            )
        })?;
        let report =
            qfs_core::DescribeReport::from_driver(driver.as_ref(), &qfs_core::Path::new(path))
                .map_err(|e| map_exec_err(&qfs_exec::map_qfs_error(&e)))?;
        serde_json::to_value(&report)
            .map_err(|e| EngineError::internal(format!("could not render describe report: {e}")))
    }

    fn build_plan(&self, statement: &str) -> Result<qfs_core::Plan, EngineError> {
        let stmt = qfs_exec::parse(statement).map_err(|e| map_exec_err(&e))?;
        qfs_exec::build_plan(&stmt, &self.engine).map_err(|e| map_exec_err(&e))
    }

    fn commit_policy(&self) -> qfs_mcp::Policy {
        // Default-deny is the law for this UNAUTHENTICATED, unattended surface: there is no
        // per-statement policy attached over MCP yet, so a write effect is refused with the policy
        // decision until a real policy is wired (a later ticket). The CLI's allow-all capability
        // shortcut is deliberately NOT reused here — an MCP tool call gets no privileged shortcut.
        qfs_mcp::default_deny_policy()
    }

    fn apply(&self, plan: &qfs_core::Plan) -> Result<(), EngineError> {
        // Offload to a dedicated OS thread so `apply_plan`'s `block_on` does not run on the serve
        // tokio runtime's reactor thread (which would panic). The commit drives the SAME runtime
        // interpreter + live driver registry the CLI `qfs run --commit` path uses.
        let result = std::thread::scope(|s| s.spawn(|| crate::commit::apply_plan(plan)).join());
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(map_exec_err(&e)),
            Err(_) => Err(EngineError::internal("the commit worker thread panicked")),
        }
    }

    fn connections(&self) -> Result<Vec<ConnectionInfo>, EngineError> {
        // Best-effort, redacted: open the SAME envelope-encrypted store `qfs connection list`
        // reads, list selectors + metadata ONLY (never credential material). If the store cannot
        // be unlocked (no passphrase / no DB), report an empty list rather than failing — the MCP
        // surface never blocks on a locked credential store, and never leaks a secret.
        let store = match crate::connection::open_store_for_commit() {
            Some(store) => store,
            None => return Ok(Vec::new()),
        };
        let records = store
            .list(None)
            .map_err(|_| EngineError::new("list_failed", "could not list connections"))?;
        Ok(records
            .into_iter()
            .map(|r| ConnectionInfo {
                driver: r.driver.0,
                connection: r.connection.as_str().to_string(),
                created_at: format_rfc3339(r.created_at),
            })
            .collect())
    }
}

/// Format a stored connection's `created_at` as RFC 3339 (plaintext metadata, no secret). Falls
/// back to the empty string on the impossible-format case rather than panicking.
fn format_rfc3339(ts: time::OffsetDateTime) -> String {
    ts.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Serve one `POST /mcp` request through the [`qfs_mcp::McpBinding`], adapting between the
/// listener's native [`qfs_http`] request/response DTOs and the pure [`qfs_http_core`] DTOs the
/// binding speaks. This is the seam the binary composes into the qfs-http listener's `Fallback`
/// (so qfs-mcp itself depends on neither qfs-http nor tokio).
#[must_use]
pub fn serve_mcp_request(
    binding: &qfs_mcp::McpBinding,
    req: &qfs_http::HttpRequest,
) -> qfs_http::HttpResponse {
    let core_req = to_core_request(req);
    let core_resp = binding.handle(&core_req);
    to_http_response(&core_resp)
}

/// Adapt the listener's [`qfs_http::HttpRequest`] onto the pure [`qfs_mcp::HttpRequest`] the
/// binding consumes (method + path-as-url + headers + body).
fn to_core_request(req: &qfs_http::HttpRequest) -> qfs_mcp::HttpRequest {
    let method = match req.method {
        qfs_http::Method::Post => qfs_mcp::HttpMethod::Post,
        qfs_http::Method::Put => qfs_mcp::HttpMethod::Put,
        qfs_http::Method::Patch => qfs_mcp::HttpMethod::Patch,
        qfs_http::Method::Delete => qfs_mcp::HttpMethod::Delete,
        // GET and any other token map to GET — the binding rejects every non-POST method anyway,
        // so the exact non-POST mapping does not matter (it only checks `== Post`).
        qfs_http::Method::Get | qfs_http::Method::Other(_) => qfs_mcp::HttpMethod::Get,
    };
    let mut core = qfs_mcp::HttpRequest::new(method, req.path.clone());
    for (k, v) in &req.headers {
        core = core.header(k.clone(), v.clone());
    }
    if !req.body.is_empty() {
        core = core.with_body(req.body.clone());
    }
    core
}

/// Adapt the binding's [`qfs_mcp::HttpResponse`] back onto the listener's
/// [`qfs_http::HttpResponse`], carrying the `content-type` header through as the response's
/// content type (defaulting to JSON).
fn to_http_response(resp: &qfs_mcp::HttpResponse) -> qfs_http::HttpResponse {
    let content_type = resp
        .header_value("content-type")
        .unwrap_or("application/json")
        .to_string();
    qfs_http::HttpResponse::new(resp.status, content_type, resp.body.clone())
}
