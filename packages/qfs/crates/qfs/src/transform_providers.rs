//! The **live** model providers behind the transform seam (blueprint §15, decision W) — the
//! deferred half of the transform epic (T4's owner-gated live run). Three hand-rolled REST
//! [`ModelProvider`] impls (Anthropic Messages, OpenAI Chat Completions, Google Gemini
//! `generateContent`), plus the [`LiveModelProvider`] dispatcher that routes a call to the impl
//! named by the transform definition's `provider` column.
//!
//! ## Where this sits (the one-seam lock is preserved)
//! These impls live in the terminal `qfs` binary — the composition leaf §15 mandates for the live
//! provider. They RECEIVE the crate-private [`CallProof`](qfs_driver_transform::CallProof) witness
//! and are invoked only through `qfs_driver_transform::call_model` (the sole funnel) from
//! [`BinaryTransformExecutor`](crate::transform::BinaryTransformExecutor). No new model-call path
//! is added: an impl cannot forge the witness, so the lock still holds.
//!
//! ## No vendor SDKs — the confined REST transport
//! Every provider builds an owned [`HttpRequest`] and sends it through the shared, synchronous
//! [`HttpClient`] seam (`qfs-driver-http`) — the SAME confined `reqwest` transport the github/slack
//! commit path uses. Vendor request/response JSON is a local shape parsed here; no vendor type
//! crosses the [`ModelProvider`] boundary (blueprint §11). Tests inject `MockHttpClient` and never
//! touch the network.
//!
//! ## Safety floor (blueprint §8)
//! - The resolved API key rides a SEPARATE `secret` parameter, never a field of [`ModelRequest`].
//!   It is written into the auth header only at wire-build time. Every auth header
//!   (`Authorization`, `x-api-key`, `x-goog-api-key`) is in the single redaction authority
//!   (`qfs_http_core::SENSITIVE_HEADERS`), so a `{req:?}` dump redacts it.
//! - [`ModelError`] reasons are built from the request *shape* + the response *status*, never from a
//!   header value or the returned text — secret-free by construction.
//! - A missing key fails closed **pre-network** (no request is ever built).
//!
//! ## Structured output (per-provider fidelity)
//! Every provider is asked to emit a JSON object (row-wise/extraction) or array of objects
//! (relation-wise) whose fields are the declared OUTPUT columns, using each vendor's native
//! JSON-output control: Anthropic a system instruction, OpenAI `response_format: json_object`,
//! Gemini `responseMimeType: application/json`. The returned text is parsed and folded onto the
//! OUTPUT schema here; the engine performs the final membership check over what we return (the
//! model's output is untrusted).

use std::sync::Arc;

use qfs_driver_http::{HttpClient, HttpMethod, HttpRequest, HttpResponse};
use qfs_driver_transform::{CallProof, ModelError, ModelProvider, ModelRequest};
use qfs_types::{ColumnType, Fields, Row, RowBatch, Schema, TransformMode, Value};

/// Anthropic Messages API endpoint (blueprint §11: hand-rolled REST, no `anthropic-sdk`).
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
/// Pinned Anthropic API version (ticket consideration: hand-rolled REST needs a pinned version
/// header so a provider-side default shift cannot silently change the wire contract).
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// OpenAI Chat Completions endpoint. Chat Completions (not `/v1/responses`) is the recorded pick:
/// it is the widest-supported OpenAI surface, has a stable `response_format: json_object` control,
/// and its `{choices:[{message:{content}}]}` shape is trivial to fold onto the OUTPUT schema.
const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
/// Google Gemini `generateContent` base (the model id is spliced into the path).
const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// The default output-token ceiling — cost control for an owner-attended tryout (ticket
/// consideration: cap `max_tokens` so a runaway generation cannot spend without bound).
const DEFAULT_MAX_TOKENS: u32 = 2048;
/// The bounded retry budget: the initial attempt plus at most two retries on a transient status.
const MAX_ATTEMPTS: u32 = 3;
/// The upper bound on a honored `Retry-After` sleep (seconds) — a hostile/absurd header cannot wedge
/// the calling thread (blueprint §7: a hung dependency must fail closed, not block forever).
const MAX_RETRY_SLEEP_SECS: u64 = 30;

/// The live provider dispatcher the binary registers in place of `UnconfiguredProvider`. Holds the
/// one shared HTTP transport and routes each [`ModelProvider::call`] to the impl named by the
/// definition's `provider` column (`anthropic` / `openai` / `google`). An unknown provider keeps
/// failing closed as [`ModelError::Unconfigured`] — exactly the old default's behavior, so a typo'd
/// provider still refuses at COMMIT rather than pretending.
pub struct LiveModelProvider {
    http: Arc<dyn HttpClient>,
}

impl LiveModelProvider {
    /// Build the dispatcher over an injected HTTP transport (the confined `ReqwestClient` in the
    /// binary; a `MockHttpClient` in tests).
    #[must_use]
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }
}

impl ModelProvider for LiveModelProvider {
    fn call(
        &self,
        req: &ModelRequest<'_>,
        secret: Option<&str>,
        _proof: &CallProof,
    ) -> Result<RowBatch, ModelError> {
        match req.provider {
            "anthropic" => anthropic_call(self.http.as_ref(), req, secret),
            "openai" => openai_call(self.http.as_ref(), req, secret),
            "google" => google_call(self.http.as_ref(), req, secret),
            other => Err(ModelError::Unconfigured {
                provider: other.to_string(),
            }),
        }
    }
}

// ---- document (Extraction) input --------------------------------------------------------------

/// Per-provider inline-document byte caps. A document over the cap fails **pre-network** (no request
/// is built), so the limit is surfaced instead of discovered as a provider 413 mid-commit.
/// Conservative vs the published limits (Anthropic ~32 MB PDF, Gemini ~20 MB inlineData).
const ANTHROPIC_DOC_MAX_BYTES: usize = 32 * 1024 * 1024;
const OPENAI_DOC_MAX_BYTES: usize = 32 * 1024 * 1024;
const GEMINI_DOC_MAX_BYTES: usize = 20 * 1024 * 1024;

fn document_cap(provider: &str) -> usize {
    match provider {
        "google" => GEMINI_DOC_MAX_BYTES,
        "openai" => OPENAI_DOC_MAX_BYTES,
        _ => ANTHROPIC_DOC_MAX_BYTES,
    }
}

/// The Extraction-mode document: the single input row's `Bytes` value + a sniffed media type. `None`
/// when the input is not a single non-empty bytes cell (so the caller falls back to the JSON text
/// user-turn). Extraction is *derived* from a single `bytes` INPUT column (`derive_mode`), so this is
/// the bytes-to-provider document leg the mode was built for.
fn extraction_document(input: &RowBatch) -> Option<(&'static str, &[u8])> {
    let row = input.rows.first()?;
    let bytes = row.values.iter().find_map(|v| match v {
        Value::Bytes(b) if !b.is_empty() => Some(b.as_slice()),
        _ => None,
    })?;
    Some((sniff_document_mime(bytes), bytes))
}

/// Best-effort media-type sniff for a document input: PDF / PNG / JPEG by magic bytes, else PDF
/// (Extraction's flagship input is a PDF document, so an unsniffable blob is treated as one).
fn sniff_document_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else {
        "application/pdf"
    }
}

/// Base64-encode document bytes for a provider's inline document field.
fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Fail closed pre-network if a document exceeds the provider's inline cap, naming the size + cap so
/// the error is actionable (the caller never builds the request).
fn check_document_size(provider: &str, len: usize) -> Result<(), ModelError> {
    let cap = document_cap(provider);
    if len > cap {
        return Err(ModelError::Provider {
            reason: format!(
                "document input is {len} bytes, over provider '{provider}' {cap}-byte inline \
                 document cap; shrink or split the document (chunking is out of scope here)"
            ),
        });
    }
    Ok(())
}

/// A short user nudge accompanying a document part — the OUTPUT contract already rides the system
/// instruction, so this only points the model at the attached document.
const DOC_PROMPT: &str = "Extract the requested fields from the attached document.";

// ---- per-provider wire shapes ---------------------------------------------------------------

/// Anthropic Messages API: `x-api-key` + pinned `anthropic-version`, a `system` instruction pinning
/// the JSON OUTPUT contract, and the input rows as the user turn. The reply's `content[].text`
/// parts are concatenated and folded onto the OUTPUT schema.
fn anthropic_call(
    http: &dyn HttpClient,
    req: &ModelRequest<'_>,
    secret: Option<&str>,
) -> Result<RowBatch, ModelError> {
    let key = require_key(req.provider, secret)?;
    // Extraction with a bytes input is a DOCUMENT turn (a base64 `document` content block); every
    // other mode stays the JSON-text user-turn. The OUTPUT contract rides the `system` instruction.
    let content = match (req.mode, extraction_document(req.input)) {
        (TransformMode::Extraction, Some((mime, bytes))) => {
            check_document_size(req.provider, bytes.len())?;
            serde_json::json!([
                { "type": "document",
                  "source": { "type": "base64", "media_type": mime, "data": b64(bytes) } },
                { "type": "text", "text": DOC_PROMPT },
            ])
        }
        _ => serde_json::Value::String(render_input_json(req.input)),
    };
    let body = serde_json::json!({
        "model": req.model,
        "max_tokens": max_tokens_for(req.effort),
        "system": output_instruction(req.output, req.mode),
        "messages": [{ "role": "user", "content": content }],
    });
    let request = HttpRequest::new(HttpMethod::Post, ANTHROPIC_URL)
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .with_body(encode_body(&body));
    let resp = send_with_retry(http, request, req.provider)?;
    let json = decode_json(&resp, req.provider)?;
    let text = concat_text_parts(json.pointer("/content"), "text");
    require_completion(&text, req.provider)?;
    rows_from_json_text(req.output, &text, req.mode, req.provider)
}

/// OpenAI Chat Completions: bearer auth, `response_format: json_object`, a system instruction + the
/// input rows as the user turn. The reply's `choices[0].message.content` is the JSON text.
fn openai_call(
    http: &dyn HttpClient,
    req: &ModelRequest<'_>,
    secret: Option<&str>,
) -> Result<RowBatch, ModelError> {
    let key = require_key(req.provider, secret)?;
    // Extraction with a bytes input attaches a base64 `file` content part (a data: URL); other modes
    // stay the JSON-text user-turn.
    let user_content = match (req.mode, extraction_document(req.input)) {
        (TransformMode::Extraction, Some((mime, bytes))) => {
            check_document_size(req.provider, bytes.len())?;
            serde_json::json!([
                { "type": "file",
                  "file": { "filename": "input.pdf",
                            "file_data": format!("data:{mime};base64,{}", b64(bytes)) } },
                { "type": "text", "text": DOC_PROMPT },
            ])
        }
        _ => serde_json::Value::String(render_input_json(req.input)),
    };
    let body = serde_json::json!({
        "model": req.model,
        // `max_completion_tokens`, NOT `max_tokens`: the chat-completions API accepts the former on
        // BOTH reasoning (o-series, gpt-5) and non-reasoning models, while a reasoning model rejects
        // `max_tokens` with HTTP 400 (round-7 defect: `gpt-5-mini` returned an unexplained 400).
        "max_completion_tokens": max_tokens_for(req.effort),
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": output_instruction(req.output, req.mode) },
            { "role": "user", "content": user_content },
        ],
    });
    let request = HttpRequest::new(HttpMethod::Post, OPENAI_URL)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .with_body(encode_body(&body));
    let resp = send_with_retry(http, request, req.provider)?;
    let json = decode_json(&resp, req.provider)?;
    let text = json
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    require_completion(&text, req.provider)?;
    rows_from_json_text(req.output, &text, req.mode, req.provider)
}

/// Google Gemini `generateContent`: the API key on `x-goog-api-key` (redacted), the OUTPUT contract
/// as a `system_instruction`, `responseMimeType: application/json`, and the input rows as the user
/// turn. The reply's `candidates[0].content.parts[].text` is concatenated.
fn google_call(
    http: &dyn HttpClient,
    req: &ModelRequest<'_>,
    secret: Option<&str>,
) -> Result<RowBatch, ModelError> {
    let key = require_key(req.provider, secret)?;
    // Extraction with a bytes input rides a Gemini `inline_data` part; other modes stay text parts.
    let user_parts = match (req.mode, extraction_document(req.input)) {
        (TransformMode::Extraction, Some((mime, bytes))) => {
            check_document_size(req.provider, bytes.len())?;
            serde_json::json!([
                { "inline_data": { "mime_type": mime, "data": b64(bytes) } },
                { "text": DOC_PROMPT },
            ])
        }
        _ => serde_json::json!([{ "text": render_input_json(req.input) }]),
    };
    let body = serde_json::json!({
        "system_instruction": { "parts": [{ "text": output_instruction(req.output, req.mode) }] },
        "contents": [{ "role": "user", "parts": user_parts }],
        "generationConfig": {
            "maxOutputTokens": max_tokens_for(req.effort),
            "responseMimeType": "application/json",
        },
    });
    // The key rides the `x-goog-api-key` header (redacted), never the URL query — so it cannot leak
    // through a URL that error/log surfaces DO carry.
    let url = format!("{GEMINI_BASE}/{}:generateContent", req.model);
    let request = HttpRequest::new(HttpMethod::Post, url)
        .header("x-goog-api-key", key)
        .header("content-type", "application/json")
        .with_body(encode_body(&body));
    let resp = send_with_retry(http, request, req.provider)?;
    let json = decode_json(&resp, req.provider)?;
    let text = concat_text_parts(json.pointer("/candidates/0/content/parts"), "text");
    require_completion(&text, req.provider)?;
    rows_from_json_text(req.output, &text, req.mode, req.provider)
}

// ---- shared request/response machinery ------------------------------------------------------

/// Guard against an EMPTY completion before the JSON parse. A reasoning model can spend its whole
/// output budget on thinking tokens and return no visible text; that previously surfaced as the
/// misleading "did not return JSON matching the declared OUTPUT schema" (round-7 Gemini
/// `gemini-flash-latest` at `effort low`). Say what actually happened and what to do.
///
/// # Errors
/// [`ModelError::Provider`] naming the empty-completion cause and the remedy.
fn require_completion(text: &str, provider: &str) -> Result<(), ModelError> {
    if text.trim().is_empty() {
        return Err(ModelError::Provider {
            reason: format!(
                "provider '{provider}' returned an empty completion — a reasoning model can spend \
                 the whole token budget on thinking and leave no output; raise the transform's \
                 `effort` (e.g. `effort 'high'`) so output room remains after thinking"
            ),
        });
    }
    Ok(())
}

/// The resolved API key, or a **pre-network** fail-closed error. Every live provider needs a
/// credential; a `None` secret (no `secret_ref` on the def, or one that resolved to nothing) refuses
/// before any request is built — the model is never contacted anonymously.
fn require_key<'a>(provider: &str, secret: Option<&'a str>) -> Result<&'a str, ModelError> {
    secret
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ModelError::Provider {
            reason: format!(
                "provider '{provider}' requires an API key; set the transform's secret_ref \
             (env:<VAR> or vault:<driver>/<connection>)"
            ),
        })
}

/// Map the definition's `effort` hint to a bounded output-token ceiling. Unknown/absent ⇒ the
/// default. Every arm stays under a hard cap so an owner-attended live round cannot run away.
///
/// The ceilings leave OUTPUT room after a reasoning model's thinking tokens (round-7 defect: `low`
/// was 256, which a reasoning Gemini/o-series model spends almost entirely on `thoughtsTokenCount`,
/// returning no visible completion). A ceiling is not a target — a non-reasoning model still emits
/// only what it needs and stops, so raising it costs those models nothing.
fn max_tokens_for(effort: Option<&str>) -> u32 {
    match effort {
        Some("low") => 1024,
        Some("high") => 4096,
        _ => DEFAULT_MAX_TOKENS,
    }
}

/// Serialize a request body to bytes. `serde_json::Value` always serializes, so the fallback is
/// unreachable; stay total rather than `unwrap`.
fn encode_body(body: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(body).unwrap_or_default()
}

/// Send with a **bounded** retry on a transient status (429 / 5xx), honoring `Retry-After` (clamped
/// to [`MAX_RETRY_SLEEP_SECS`]). A transport failure or a terminal status (4xx / other) is NOT
/// retried — it maps straight to a secret-free [`ModelError`]. Only 429/5xx are retried because the
/// server explicitly signaled the request was not processed, so a POST re-send is side-effect-safe.
fn send_with_retry(
    http: &dyn HttpClient,
    request: HttpRequest,
    provider: &str,
) -> Result<HttpResponse, ModelError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match http.send(&request) {
            Ok(resp) if resp.is_success() => return Ok(resp),
            Ok(resp) if is_transient(resp.status) && attempt < MAX_ATTEMPTS => {
                sleep_before_retry(retry_after_secs(&resp), attempt);
            }
            Ok(resp) => {
                return Err(ModelError::Provider {
                    reason: format!(
                        "provider '{provider}' returned HTTP {} (no usable completion)",
                        resp.status
                    ),
                });
            }
            Err(err) => {
                // `HttpError::Display` is secret-free by the driver's contract (method + URL +
                // class reason only), so it is a safe transport-class reason.
                return Err(ModelError::Provider {
                    reason: format!("provider '{provider}' transport failure: {err}"),
                });
            }
        }
    }
}

/// A transient (retry-worthy) status: rate-limit (429) or any 5xx server error.
fn is_transient(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// The `Retry-After` delay in whole seconds, if the response carries a numeric one. A date-form
/// `Retry-After` is ignored (we fall back to backoff) — never parsed into an unbounded wait.
fn retry_after_secs(resp: &HttpResponse) -> Option<u64> {
    resp.header_value("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// Sleep before a retry: the honored `Retry-After` (clamped), else a small linear backoff. Kept
/// tiny and bounded so a wedged provider cannot hold the commit thread open.
fn sleep_before_retry(retry_after: Option<u64>, attempt: u32) {
    let secs = retry_after
        .unwrap_or(u64::from(attempt))
        .min(MAX_RETRY_SLEEP_SECS);
    std::thread::sleep(std::time::Duration::from_secs(secs));
}

/// Decode a successful response body as JSON, or a secret-free decode error (the body is data, never
/// a credential, but we do not echo it into the reason).
fn decode_json(resp: &HttpResponse, provider: &str) -> Result<serde_json::Value, ModelError> {
    serde_json::from_slice(&resp.body).map_err(|_| ModelError::Provider {
        reason: format!("provider '{provider}' returned a non-JSON body"),
    })
}

/// Concatenate the `text` field of every element of a `parts`/`content` array (Anthropic content
/// blocks, Gemini parts). A missing/!array node yields the empty string.
fn concat_text_parts(node: Option<&serde_json::Value>, field: &str) -> String {
    node.and_then(serde_json::Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get(field).and_then(serde_json::Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

// ---- OUTPUT-schema folding ------------------------------------------------------------------

/// The system instruction pinning the JSON OUTPUT contract: name every OUTPUT column and its type,
/// and demand a bare JSON object (row-wise/extraction) or array of objects (relation-wise) with no
/// prose. This is the portable structured-output request; each provider ALSO sets its native
/// JSON-mode flag, so the model is constrained on both axes.
fn output_instruction(output: &Schema, mode: TransformMode) -> String {
    let fields = output
        .columns
        .iter()
        .map(|c| format!("\"{}\" ({})", c.name, c.ty.type_token()))
        .collect::<Vec<_>>()
        .join(", ");
    let shape = match mode {
        TransformMode::RelationWise => "a JSON array of objects",
        TransformMode::RowWise | TransformMode::Extraction => "a single JSON object",
    };
    format!(
        "Respond with ONLY {shape}, no prose and no code fences. \
         Each object must have exactly these fields: {fields}."
    )
}

/// Render the input batch as a JSON array of objects (column name → value) for the user turn.
fn render_input_json(batch: &RowBatch) -> String {
    let rows: Vec<serde_json::Value> = batch
        .rows
        .iter()
        .map(|row| {
            let obj: serde_json::Map<String, serde_json::Value> = batch
                .schema
                .columns
                .iter()
                .zip(&row.values)
                .map(|(col, val)| (col.name.clone(), value_to_json(val)))
                .collect();
            serde_json::Value::Object(obj)
        })
        .collect();
    serde_json::Value::Array(rows).to_string()
}

/// Parse the model's returned text into rows shaped for the OUTPUT schema. Tolerates a ```json```
/// code fence the model may add despite the instruction. An array becomes one row per element; a
/// bare object becomes one row. Each OUTPUT column is read by name and coerced to its declared type;
/// a missing field degrades to `Null` (the engine's OUTPUT membership check is the final gate).
fn rows_from_json_text(
    output: &Schema,
    text: &str,
    mode: TransformMode,
    provider: &str,
) -> Result<RowBatch, ModelError> {
    let cleaned = strip_code_fence(text);
    let parsed: serde_json::Value =
        serde_json::from_str(cleaned).map_err(|_| ModelError::Provider {
            reason: format!(
                "provider '{provider}' did not return JSON matching the declared OUTPUT schema"
            ),
        })?;
    let objects: Vec<&serde_json::Value> = match &parsed {
        serde_json::Value::Array(items) => items.iter().collect(),
        // A single object (or, defensively for relation-wise, a lone object) is one row.
        other => vec![other],
    };
    let _ = mode; // shape is advisory to the model; we accept object-or-array from any mode.
    let rows = objects
        .iter()
        .map(|obj| row_for_output(output, obj))
        .collect();
    Ok(RowBatch::new(output.clone(), rows))
}

/// Build one OUTPUT row from a parsed JSON object: each declared column read by name and coerced to
/// its type, missing keys `Null`.
fn row_for_output(output: &Schema, obj: &serde_json::Value) -> Row {
    let values = output
        .columns
        .iter()
        .map(|col| match obj.get(&col.name) {
            Some(v) => coerce_to_column(v, &col.ty),
            None => Value::Null,
        })
        .collect();
    Row::new(values)
}

/// Strip a leading/trailing Markdown code fence (```json … ``` or ``` … ```) the model may wrap its
/// JSON in, returning the inner payload trimmed.
fn strip_code_fence(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    // Drop the optional language tag on the opening fence line.
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let rest = rest.trim_start_matches(['\r', '\n']);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/// Coerce a parsed JSON value to the declared OUTPUT column type. Scalars parse/stringify to the
/// target; structured columns (`struct`/`array`/`json`/`unknown`) keep the JSON tree via
/// [`json_to_value`]. A shape that cannot fit the scalar target degrades to `Null` rather than
/// failing the whole batch (the engine's typeck is the authority).
fn coerce_to_column(v: &serde_json::Value, ty: &ColumnType) -> Value {
    if v.is_null() {
        return Value::Null;
    }
    match ty {
        ColumnType::Text | ColumnType::Decimal | ColumnType::Uuid => match v {
            serde_json::Value::String(s) => Value::Text(s.clone()),
            other => Value::Text(other.to_string()),
        },
        ColumnType::Int | ColumnType::Timestamp | ColumnType::Date => v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
            .map_or(Value::Null, Value::Int),
        ColumnType::Float => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
            .map_or(Value::Null, Value::Float),
        ColumnType::Bool => v
            .as_bool()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<bool>().ok()))
            .map_or(Value::Null, Value::Bool),
        // Structured / opaque / any future column type keeps the JSON tree faithfully.
        _ => json_to_value(v),
    }
}

/// A minimal, self-contained `serde_json::Value → qfs Value` map (the binary does not depend on
/// `qfs-codec`, whose `json_to_value` this mirrors). Objects become self-describing structs, arrays
/// homogeneous collections, integral numbers `Int` else `Float`.
fn json_to_value(node: &serde_json::Value) -> Value {
    match node {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .or_else(|| n.as_f64().map(Value::Float))
            .unwrap_or_else(|| Value::Json(node.clone())),
        serde_json::Value::String(s) => Value::Text(s.clone()),
        serde_json::Value::Array(items) => Value::Array(items.iter().map(json_to_value).collect()),
        serde_json::Value::Object(map) => Value::Struct(Fields::new(
            map.iter()
                .map(|(k, child)| (k.clone(), json_to_value(child)))
                .collect(),
        )),
    }
}

/// A `qfs Value → serde_json::Value` map for rendering input rows into the request body.
fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) | Value::Timestamp(i) => serde_json::Value::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        // Bytes are base64-encoded so a binary input column is a legible JSON string (the
        // bytes-to-provider leg proper is a downstream ticket; text-gen inputs stay lossless here).
        Value::Bytes(b) => {
            use base64::Engine as _;
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b))
        }
        Value::Struct(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect(),
        ),
        Value::Array(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Json(j) => j.clone(),
        // Any future value variant renders as JSON null rather than failing the request build.
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_driver_http::MockHttpClient;
    use qfs_driver_transform::call_model;
    use qfs_types::Column;

    fn output_schema() -> Schema {
        Schema::new(vec![Column::new("label", ColumnType::Text, true)])
    }

    fn input_batch() -> RowBatch {
        RowBatch::new(
            Schema::new(vec![Column::new("body", ColumnType::Text, true)]),
            vec![Row::new(vec![Value::Text("hello world".into())])],
        )
    }

    fn request<'a>(provider: &'a str, output: &'a Schema, input: &'a RowBatch) -> ModelRequest<'a> {
        ModelRequest {
            name: "classify",
            provider,
            model: "test-model",
            effort: Some("medium"),
            mode: TransformMode::RowWise,
            output,
            input,
        }
    }

    // ---- request-shape golden tests (per provider) ----

    #[test]
    fn anthropic_builds_the_pinned_messages_request_with_a_redacted_key() {
        let output = output_schema();
        let input = input_batch();
        let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(
            200,
            br#"{"content":[{"type":"text","text":"{\"label\":\"greeting\"}"}]}"#.to_vec(),
        )));
        let provider = LiveModelProvider::new(mock.clone());
        let out = call_model(
            &provider,
            &request("anthropic", &output, &input),
            Some("sk-secret"),
        )
        .unwrap();
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.rows[0].values[0], Value::Text("greeting".into()));

        let rec = mock.recorded();
        assert_eq!(rec.len(), 1);
        let req = &rec[0];
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, ANTHROPIC_URL);
        assert_eq!(req.header_value("x-api-key"), Some("sk-secret"));
        assert_eq!(
            req.header_value("anthropic-version"),
            Some(ANTHROPIC_VERSION)
        );
        // The model selector is in the body; the OUTPUT contract names the declared column.
        let body: serde_json::Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["model"], "test-model");
        assert!(body["system"].as_str().unwrap().contains("\"label\""));
        // The key is NEVER in the Debug rendering (single redaction authority covers x-api-key).
        let dbg = format!("{req:?}");
        assert!(!dbg.contains("sk-secret"), "key leaked in Debug: {dbg}");
    }

    #[test]
    fn openai_builds_a_bearer_json_object_request() {
        let output = output_schema();
        let input = input_batch();
        let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(
            200,
            br#"{"choices":[{"message":{"content":"{\"label\":\"greeting\"}"}}]}"#.to_vec(),
        )));
        let provider = LiveModelProvider::new(mock.clone());
        let out = call_model(
            &provider,
            &request("openai", &output, &input),
            Some("sk-open"),
        )
        .unwrap();
        assert_eq!(out.rows[0].values[0], Value::Text("greeting".into()));

        let req = &mock.recorded()[0];
        assert_eq!(req.url, OPENAI_URL);
        assert_eq!(req.header_value("authorization"), Some("Bearer sk-open"));
        let body: serde_json::Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["response_format"]["type"], "json_object");
        assert!(!format!("{req:?}").contains("sk-open"));
    }

    #[test]
    fn google_builds_a_generatecontent_request_with_the_key_on_the_header_not_the_url() {
        let output = output_schema();
        let input = input_batch();
        let mock = Arc::new(
            MockHttpClient::new().with_response(HttpResponse::new(
                200,
                br#"{"candidates":[{"content":{"parts":[{"text":"{\"label\":\"greeting\"}"}]}}]}"#
                    .to_vec(),
            )),
        );
        let provider = LiveModelProvider::new(mock.clone());
        let out = call_model(
            &provider,
            &request("google", &output, &input),
            Some("goog-key"),
        )
        .unwrap();
        assert_eq!(out.rows[0].values[0], Value::Text("greeting".into()));

        let req = &mock.recorded()[0];
        assert!(
            req.url.ends_with("/test-model:generateContent"),
            "{}",
            req.url
        );
        // The key rides the header, never the URL query — so a logged URL cannot carry it.
        assert!(
            !req.url.contains("goog-key"),
            "key leaked into URL: {}",
            req.url
        );
        assert_eq!(req.header_value("x-goog-api-key"), Some("goog-key"));
        assert!(!format!("{req:?}").contains("goog-key"));
    }

    // ---- Extraction (PDF document) input, ticket 20260711121530 ----

    const FAKE_PDF: &[u8] = b"%PDF-1.7\nfake invoice document bytes\n%%EOF";

    fn extraction_input(bytes: &[u8]) -> RowBatch {
        RowBatch::new(
            Schema::new(vec![Column::new("blob", ColumnType::Bytes, true)]),
            vec![Row::new(vec![Value::Bytes(bytes.to_vec())])],
        )
    }

    fn extraction_request<'a>(
        provider: &'a str,
        output: &'a Schema,
        input: &'a RowBatch,
    ) -> ModelRequest<'a> {
        ModelRequest {
            name: "extract",
            provider,
            model: "test-model",
            effort: Some("medium"),
            mode: TransformMode::Extraction,
            output,
            input,
        }
    }

    /// The base64 of the fixture PDF — asserted present in each provider's document field.
    fn fake_pdf_b64() -> String {
        b64(FAKE_PDF)
    }

    #[test]
    fn anthropic_extraction_emits_a_base64_pdf_document_block() {
        let output = output_schema();
        let input = extraction_input(FAKE_PDF);
        let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(
            200,
            br#"{"content":[{"type":"text","text":"{\"label\":\"invoice\"}"}]}"#.to_vec(),
        )));
        let provider = LiveModelProvider::new(mock.clone());
        let out = call_model(
            &provider,
            &extraction_request("anthropic", &output, &input),
            Some("sk-secret"),
        )
        .unwrap();
        assert_eq!(out.rows[0].values[0], Value::Text("invoice".into()));

        let body: serde_json::Value =
            serde_json::from_slice(mock.recorded()[0].body.as_ref().unwrap()).unwrap();
        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["type"], "document");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "application/pdf");
        assert_eq!(
            block["source"]["data"],
            fake_pdf_b64(),
            "the PDF bytes ride the document block base64, not a JSON-text field"
        );
    }

    #[test]
    fn openai_extraction_emits_a_base64_file_part() {
        let output = output_schema();
        let input = extraction_input(FAKE_PDF);
        let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(
            200,
            br#"{"choices":[{"message":{"content":"{\"label\":\"invoice\"}"}}]}"#.to_vec(),
        )));
        let provider = LiveModelProvider::new(mock.clone());
        call_model(
            &provider,
            &extraction_request("openai", &output, &input),
            Some("sk-open"),
        )
        .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(mock.recorded()[0].body.as_ref().unwrap()).unwrap();
        let part = &body["messages"][1]["content"][0];
        assert_eq!(part["type"], "file");
        let data_url = part["file"]["file_data"].as_str().unwrap();
        assert!(data_url.starts_with("data:application/pdf;base64,"));
        assert!(data_url.ends_with(&fake_pdf_b64()));
    }

    #[test]
    fn google_extraction_emits_an_inline_data_part() {
        let output = output_schema();
        let input = extraction_input(FAKE_PDF);
        let mock = Arc::new(
            MockHttpClient::new().with_response(HttpResponse::new(
                200,
                br#"{"candidates":[{"content":{"parts":[{"text":"{\"label\":\"invoice\"}"}]}}]}"#
                    .to_vec(),
            )),
        );
        let provider = LiveModelProvider::new(mock.clone());
        call_model(
            &provider,
            &extraction_request("google", &output, &input),
            Some("goog-key"),
        )
        .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(mock.recorded()[0].body.as_ref().unwrap()).unwrap();
        let part = &body["contents"][0]["parts"][0];
        assert_eq!(part["inline_data"]["mime_type"], "application/pdf");
        assert_eq!(part["inline_data"]["data"], fake_pdf_b64());
    }

    #[test]
    fn an_oversized_document_fails_pre_network_naming_the_cap() {
        let output = output_schema();
        // One byte over Gemini's 20 MiB inline cap.
        let huge = vec![b'%'; GEMINI_DOC_MAX_BYTES + 1];
        // Give it a PDF header so it sniffs as a document, not text.
        let mut pdf = b"%PDF-1.7".to_vec();
        pdf.extend_from_slice(&huge);
        let input = extraction_input(&pdf);
        let mock = Arc::new(MockHttpClient::new());
        let provider = LiveModelProvider::new(mock.clone());
        let err = call_model(
            &provider,
            &extraction_request("google", &output, &input),
            Some("goog-key"),
        )
        .unwrap_err();
        match err {
            qfs_driver_transform::ModelError::Provider { reason } => {
                assert!(reason.contains("over provider") && reason.contains("inline"));
            }
            other => panic!("expected a pre-network size error, got {other:?}"),
        }
        assert!(
            mock.recorded().is_empty(),
            "an oversized document must fail BEFORE any request is built"
        );
    }

    // ---- fail-closed / error mapping ----

    #[test]
    fn a_missing_key_fails_closed_before_any_request_is_built() {
        let output = output_schema();
        let input = input_batch();
        let mock = Arc::new(MockHttpClient::new());
        let provider = LiveModelProvider::new(mock.clone());
        let err = call_model(&provider, &request("anthropic", &output, &input), None).unwrap_err();
        assert!(matches!(err, ModelError::Provider { .. }));
        assert!(err.to_string().contains("requires an API key"));
        assert!(mock.recorded().is_empty(), "no request on a missing key");
    }

    #[test]
    fn an_unknown_provider_stays_unconfigured() {
        let output = output_schema();
        let input = input_batch();
        let provider = LiveModelProvider::new(Arc::new(MockHttpClient::new()));
        let err =
            call_model(&provider, &request("bedrock", &output, &input), Some("k")).unwrap_err();
        assert!(matches!(err, ModelError::Unconfigured { provider } if provider == "bedrock"));
    }

    #[test]
    fn a_client_error_status_maps_to_a_secret_free_provider_error() {
        let output = output_schema();
        let input = input_batch();
        let mock =
            Arc::new(MockHttpClient::new().with_response(HttpResponse::new(401, Vec::new())));
        let provider = LiveModelProvider::new(mock);
        let err = call_model(
            &provider,
            &request("openai", &output, &input),
            Some("sk-bad"),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("401"), "{msg}");
        assert!(!msg.contains("sk-bad"), "secret leaked into error: {msg}");
    }

    #[test]
    fn a_non_json_body_maps_to_a_secret_free_error() {
        let output = output_schema();
        let input = input_batch();
        let mock = Arc::new(
            MockHttpClient::new().with_response(HttpResponse::new(200, b"<html>".to_vec())),
        );
        let provider = LiveModelProvider::new(mock);
        let err =
            call_model(&provider, &request("openai", &output, &input), Some("sk")).unwrap_err();
        assert!(err.to_string().contains("non-JSON"));
    }

    // ---- OUTPUT-schema folding ----

    #[test]
    fn relation_wise_parses_an_array_into_multiple_rows() {
        let output = Schema::new(vec![
            Column::new("sku", ColumnType::Text, true),
            Column::new("qty", ColumnType::Int, true),
        ]);
        let batch = rows_from_json_text(
            &output,
            r#"[{"sku":"a","qty":2},{"sku":"b","qty":5}]"#,
            TransformMode::RelationWise,
            "openai",
        )
        .unwrap();
        assert_eq!(batch.rows.len(), 2);
        assert_eq!(batch.rows[0].values[0], Value::Text("a".into()));
        assert_eq!(batch.rows[1].values[1], Value::Int(5));
    }

    #[test]
    fn a_code_fence_wrapped_object_still_parses() {
        let output = output_schema();
        let batch = rows_from_json_text(
            &output,
            "```json\n{\"label\":\"x\"}\n```",
            TransformMode::RowWise,
            "anthropic",
        )
        .unwrap();
        assert_eq!(batch.rows[0].values[0], Value::Text("x".into()));
    }

    #[test]
    fn a_missing_output_field_degrades_to_null() {
        let output = Schema::new(vec![
            Column::new("label", ColumnType::Text, true),
            Column::new("score", ColumnType::Float, true),
        ]);
        let batch = rows_from_json_text(
            &output,
            r#"{"label":"x"}"#,
            TransformMode::RowWise,
            "openai",
        )
        .unwrap();
        assert_eq!(batch.rows[0].values[0], Value::Text("x".into()));
        assert_eq!(batch.rows[0].values[1], Value::Null);
    }

    #[test]
    fn scalar_coercion_stringifies_and_parses_across_types() {
        assert_eq!(
            coerce_to_column(&serde_json::json!(42), &ColumnType::Text),
            Value::Text("42".into())
        );
        assert_eq!(
            coerce_to_column(&serde_json::json!("7"), &ColumnType::Int),
            Value::Int(7)
        );
        assert_eq!(
            coerce_to_column(&serde_json::json!(true), &ColumnType::Bool),
            Value::Bool(true)
        );
        assert_eq!(
            coerce_to_column(&serde_json::json!(null), &ColumnType::Int),
            Value::Null
        );
    }

    // ---- retry decision (pure, no sleeps) ----

    #[test]
    fn transient_status_and_retry_after_are_classified() {
        assert!(is_transient(429));
        assert!(is_transient(503));
        assert!(!is_transient(200));
        assert!(!is_transient(401));
        let resp = HttpResponse::new(429, Vec::new()).header("retry-after", "5");
        assert_eq!(retry_after_secs(&resp), Some(5));
        // A date-form Retry-After is ignored (never an unbounded parse).
        let dated = HttpResponse::new(429, Vec::new())
            .header("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(retry_after_secs(&dated), None);
    }

    #[test]
    fn effort_maps_to_a_bounded_token_ceiling() {
        // Ceilings leave output room after a reasoning model's thinking tokens (round-7):
        // `low` is 1024 (was 256, which a reasoning model spent entirely on thinking), high 4096.
        assert_eq!(max_tokens_for(Some("low")), 1024);
        assert_eq!(max_tokens_for(Some("high")), 4096);
        assert_eq!(max_tokens_for(Some("medium")), DEFAULT_MAX_TOKENS);
        assert_eq!(max_tokens_for(None), DEFAULT_MAX_TOKENS);
        assert!(
            max_tokens_for(Some("low")) >= 1024,
            "a reasoning model needs post-thinking room"
        );
    }

    #[test]
    fn openai_sends_max_completion_tokens_not_max_tokens() {
        // Round-7: reasoning chat-completions models reject `max_tokens` with HTTP 400 and require
        // `max_completion_tokens`, which non-reasoning models also accept.
        let output = output_schema();
        let input = input_batch();
        let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(
            200,
            br#"{"choices":[{"message":{"content":"{\"label\":\"greeting\"}"}}]}"#.to_vec(),
        )));
        let provider = LiveModelProvider::new(mock.clone());
        call_model(
            &provider,
            &request("openai", &output, &input),
            Some("sk-open"),
        )
        .unwrap();
        let req = &mock.recorded()[0];
        let body: serde_json::Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
        assert!(
            body.get("max_completion_tokens").is_some(),
            "OpenAI body carries max_completion_tokens: {body}"
        );
        assert!(
            body.get("max_tokens").is_none(),
            "OpenAI body must NOT carry the reasoning-rejected max_tokens: {body}"
        );
    }

    #[test]
    fn an_empty_completion_is_an_actionable_error_not_a_schema_mismatch() {
        // A reasoning model that spent its budget on thinking returns empty content; the error must
        // name the cause + remedy, not the misleading "did not return JSON matching schema".
        match require_completion("   ", "google") {
            Err(ModelError::Provider { reason }) => assert!(
                reason.contains("empty completion") && reason.contains("effort"),
                "the empty-completion error names the cause and the remedy: {reason}"
            ),
            other => panic!("expected an actionable Provider error, got {other:?}"),
        }
    }
}
