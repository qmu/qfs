//! The thin, **synchronous, runtime-free** HTTP transport seam this crate sends auth requests
//! through — a deliberate mirror of the t18 [`cfs_driver_http`] `HttpClient`/`HttpRequest`/
//! `HttpResponse` shape, defined **here** so `cfs-google-auth` does not depend on
//! `cfs-driver-http` (RFD-0001 §6, the runtime-leaf confinement invariant).
//!
//! ## Why a local seam instead of depending on `cfs-driver-http`
//! `cfs-driver-http` depends on `cfs-runtime` (for the `PlanApplierBridge` + the
//! `EffectError` lowering), and the workspace's confinement test requires **every**
//! `cfs-runtime` consumer to be a *leaf* — no crate may depend back onto it, or tokio could
//! transit out of the runtime, through that crate, and back into the pure spine. If
//! `cfs-google-auth` depended on `cfs-driver-http`, the latter would stop being a leaf and the
//! invariant would break. So this crate carries its own equivalent of the **pure** request/
//! response/client portion of the t18 seam (the part with no runtime in it), and the
//! consuming Gmail/Drive/Analytics drivers — which are themselves runtime leaves already
//! holding an `Arc<dyn cfs_driver_http::HttpClient>` — supply an adapter implementing
//! [`HttpExchange`] over it (a trivial DTO copy; see the crate docs). `reqwest` thus stays
//! confined to `cfs-driver-http` exactly as before; no new crate touches it.
//!
//! ## Secret discipline (RFD §10)
//! [`HttpRequest`] carries already-resolved header values (a bearer token may sit in an
//! `Authorization` header on the wire), so its `Debug` is **manual** and **redacts** the value
//! of every sensitive header — a request is never logged with a live token. This mirrors the
//! t18 redaction guarantee so the adapter is a pure shape copy with no loss of safety.

use core::fmt;

/// Header names whose *values* are redacted in every `Debug`/log rendering (case-insensitive).
/// Mirrors the t18 `SENSITIVE_HEADERS`; auth material rides in these.
pub const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
];

/// Whether a header name carries auth material (case-insensitive).
#[must_use]
pub fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
}

/// The HTTP method used by the auth flow. Only `GET` (userinfo) and `POST` (token endpoint)
/// are needed; a closed set mirroring the t18 method enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    /// `GET` — the userinfo lookup.
    Get,
    /// `POST` — the token exchange / refresh.
    Post,
}

impl HttpMethod {
    /// The uppercase wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        }
    }
}

/// One fully-described HTTP request — an owned DTO. Built by the OAuth client; carries
/// already-resolved header values, so its `Debug` redacts sensitive headers.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The fully-resolved request URL.
    pub url: String,
    /// Header `(name, value)` pairs in insertion order. Sensitive values are redacted in
    /// `Debug`/logs but present here for the wire send.
    pub headers: Vec<(String, String)>,
    /// The request body bytes, if any.
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// Construct a bodyless request.
    #[must_use]
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Builder: append a header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Builder: set the body.
    #[must_use]
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// The header value for `name` (case-insensitive), if present.
    #[must_use]
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Manual, redacting `Debug`: method, URL, header *names* (sensitive values replaced by the
/// redaction marker), and body length — never a token, never a raw body.
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let shown = if is_sensitive_header(k) {
                    cfs_secrets::REDACTED
                } else {
                    v.as_str()
                };
                (k.as_str(), shown)
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body_len", &self.body.as_ref().map_or(0, Vec::len))
            .finish()
    }
}

/// One HTTP response — an owned DTO the transport returns.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// Response header `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// The raw response body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Construct a response.
    #[must_use]
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    /// Builder: append a response header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The header value for `name` (case-insensitive), if present.
    #[must_use]
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether the status is a 2xx success.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}

/// Redacting `Debug` for a response: status, header names (`Set-Cookie` redacted), body length.
impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let shown = if is_sensitive_header(k) {
                    cfs_secrets::REDACTED
                } else {
                    v.as_str()
                };
                (k.as_str(), shown)
            })
            .collect();
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("headers", &headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// A transport failure before an HTTP status was received — secret-free (built from the request
/// shape only). Mirrors the transport class of the t18 `HttpError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("transport error for {method} {url}: {reason}")]
pub struct TransportError {
    /// The HTTP method (uppercase token).
    pub method: String,
    /// The request URL (secret-free).
    pub url: String,
    /// A secret-free reason (the transport's class, never a header value).
    pub reason: String,
}

impl TransportError {
    /// A short, stable code for structured surfaces (mirrors t18 `http_transport`).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "http_transport"
    }
}

/// The thin synchronous transport seam: the OAuth client builds an owned [`HttpRequest`] and
/// calls [`HttpExchange::exchange`]; the implementation performs the wire exchange and returns
/// an owned [`HttpResponse`] (a non-2xx status is **not** an error — it is in the response so
/// the caller can classify a 401/`invalid_grant` body) or a [`TransportError`].
///
/// `Send + Sync` so an `Arc<dyn HttpExchange>` can be shared across the runtime's blocking
/// apply threads. The Gmail/Drive/Analytics drivers implement this over their
/// `Arc<dyn cfs_driver_http::HttpClient>` with a trivial DTO copy.
pub trait HttpExchange: Send + Sync {
    /// Execute one request synchronously.
    ///
    /// # Errors
    /// [`TransportError`] if the wire exchange fails before a status is received.
    fn exchange(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

impl HttpExchange for std::sync::Arc<dyn HttpExchange> {
    fn exchange(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError> {
        (**self).exchange(req)
    }
}

/// An in-memory mock transport (tests / CI / wasm): records every request and answers from a
/// FIFO queue of scripted responses — so a test asserts the exact request shape the OAuth
/// client built (method, URL, headers, body) **without any socket**. Mirrors the t18
/// `MockHttpClient`.
#[derive(Default)]
pub struct MockExchange {
    responses: std::sync::Mutex<std::collections::VecDeque<Result<HttpResponse, TransportError>>>,
    recorded: std::sync::Mutex<Vec<HttpRequest>>,
}

impl MockExchange {
    /// An empty mock (every `exchange` after the queue drains returns a terminal transport
    /// error).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue one scripted success response (consumed FIFO).
    #[must_use]
    pub fn with_response(self, resp: HttpResponse) -> Self {
        self.push(Ok(resp));
        self
    }

    /// Queue one scripted transport error (consumed FIFO).
    #[must_use]
    pub fn with_error(self, err: TransportError) -> Self {
        self.push(Err(err));
        self
    }

    /// Queue a response after construction.
    pub fn push_response(&self, resp: HttpResponse) {
        self.push(Ok(resp));
    }

    fn push(&self, item: Result<HttpResponse, TransportError>) {
        if let Ok(mut q) = self.responses.lock() {
            q.push_back(item);
        }
    }

    /// The requests this mock received, in order — what a test asserts against.
    #[must_use]
    pub fn recorded(&self) -> Vec<HttpRequest> {
        self.recorded.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl HttpExchange for MockExchange {
    fn exchange(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError> {
        if let Ok(mut r) = self.recorded.lock() {
            r.push(req.clone());
        }
        let next = self.responses.lock().ok().and_then(|mut q| q.pop_front());
        next.unwrap_or_else(|| {
            Err(TransportError {
                method: req.method.as_str().to_string(),
                url: req.url.clone(),
                reason: "mock exhausted: no scripted response".to_string(),
            })
        })
    }
}
