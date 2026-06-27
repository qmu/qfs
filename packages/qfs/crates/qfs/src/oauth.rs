//! The `qfs serve` **OAuth-AS composition root** (t48): the binary loads/generates the
//! authorization server's ES256 signing key over the System DB and serves the three public,
//! read-only discovery routes by composing a handler into the qfs-http listener's `Fallback` (the
//! SAME seam t47 used for `POST /mcp`).
//!
//! Like the t46 session launcher, `qfs-cmd` / the spine may not depend on the concrete `qfs-store`
//! backend, so the binary owns this. The binary is also the one leaf that may own a CSPRNG: it mints
//! the new signing key's entropy from the OS and hands `qfs-oauth` only the random bytes (the same
//! entropy-injection discipline the session token uses), keeping the `qfs-oauth` domain off a
//! `rand`/`getrandom` edge.
//!
//! ## What is live this milestone (honesty)
//! Three routes are served: `/.well-known/oauth-protected-resource` (RFC 9728 PRM),
//! `/.well-known/oauth-authorization-server` (RFC 8414 AS metadata, advertising ONLY `issuer` +
//! `jwks_uri` + the static PKCE/response-type capabilities), and `/jwks.json` (the public JWKS). NO
//! tokens are issued yet (that is t49/t50); the `OauthKeyStore` + the `qfs_oauth::SigningKey` it
//! reconstructs are the key handle t49/t50 will mint/verify tokens with.
//!
//! ## Security (RFD §10)
//! The PRIVATE signing key is envelope-encrypted at rest (System-DB data-key) and is reconstructed
//! into a redacting, zeroized `Secret` only inside this module — it NEVER appears in a response body,
//! log, trace, or audit entry. Only PUBLIC JWK material is published at `/jwks.json`. Best-effort +
//! inert composition: without `QFS_PASSPHRASE` or a resolvable System DB the routes are simply not
//! served (the same posture t42/t46 took), never a panic.

use std::net::SocketAddr;
use std::sync::Arc;

use qfs_oauth::{
    sign_jws, verify_jws, AuthorizationServerMetadata, Jwk, Jwks, ProtectedResourceMetadata,
    SigningKey, ALG_ES256,
};
use qfs_secrets::Secret;
use qfs_store::OauthKeyStore;
use rand::RngCore;

/// The P-256 secret-scalar width: 32 bytes of OS entropy per generated key.
const ENTROPY_BYTES: usize = 32;

/// The RFC 9728 Protected Resource Metadata well-known path.
const PRM_PATH: &str = "/.well-known/oauth-protected-resource";
/// The RFC 8414 Authorization Server Metadata well-known path.
const ASM_PATH: &str = "/.well-known/oauth-authorization-server";
/// The JWKS document path.
const JWKS_PATH: &str = "/jwks.json";

/// The pre-rendered JSON bodies for the three public discovery routes, built once at boot and shared
/// (behind an `Arc`) across the listener's connections. Holds NO secret — only public documents.
pub struct OauthRoutes {
    prm: Vec<u8>,
    asm: Vec<u8>,
    jwks: Vec<u8>,
}

impl OauthRoutes {
    /// Serve one of the three discovery routes if `req` matches (a `GET` on a well-known path);
    /// otherwise `None` (the listener falls through to the next handler / 404). The composed
    /// `Fallback` calls this first.
    #[must_use]
    pub fn handle(&self, req: &qfs_http::HttpRequest) -> Option<qfs_http::HttpResponse> {
        // Read-only discovery: only GET is answered; any other method falls through.
        if req.method != qfs_http::Method::Get {
            return None;
        }
        let body = match req.path.as_str() {
            PRM_PATH => &self.prm,
            ASM_PATH => &self.asm,
            JWKS_PATH => &self.jwks,
            _ => return None,
        };
        Some(qfs_http::HttpResponse::new(
            200,
            "application/json",
            body.clone(),
        ))
    }
}

/// Boot the OAuth AS: open the System DB, unlock (or initialize) the OAuth key envelope with
/// `QFS_PASSPHRASE`, load-or-generate the one active ES256 key, self-verify it, and pre-render the
/// three discovery documents for `addr`'s issuer. Returns `None` (best-effort, logged) when the
/// System DB or passphrase is unavailable — the listener then simply serves no OAuth routes.
#[must_use]
pub fn boot_oauth(addr: SocketAddr) -> Option<Arc<OauthRoutes>> {
    let store = match open_key_store() {
        Ok(Some(store)) => store,
        Ok(None) => {
            tracing::info!(
                target: "qfs::oauth",
                "oauth AS not served (no System DB or QFS_PASSPHRASE); set QFS_PASSPHRASE to enable"
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(target: "qfs::oauth", error = %e, "oauth AS key store unavailable; not serving discovery routes");
            return None;
        }
    };

    let issuer = issuer_from_addr(addr);
    match build_routes_from_store(&store, &issuer) {
        Ok(routes) => Some(Arc::new(routes)),
        Err(e) => {
            tracing::warn!(target: "qfs::oauth", error = %e, "oauth AS could not initialize signing key; not serving discovery routes");
            None
        }
    }
}

/// Open the System DB at the default path and build the [`OauthKeyStore`] over its owned connection,
/// unlocking with `QFS_PASSPHRASE`. `Ok(None)` when the config home or the passphrase is absent
/// (best-effort, not an error); `Err` on a real open / unlock failure.
fn open_key_store() -> Result<Option<OauthKeyStore>, String> {
    let Some(sys) =
        crate::store::open_system_db().map_err(|e| format!("opening the system database: {e}"))?
    else {
        return Ok(None);
    };
    let pass = match std::env::var("QFS_PASSPHRASE") {
        Ok(p) if !p.is_empty() => p,
        _ => return Ok(None),
    };
    let store = OauthKeyStore::from_db(sys.into_db(), &Secret::from(pass))
        .map_err(|e| format!("unlocking the oauth key store: {e}"))?;
    Ok(Some(store))
}

/// Load the active key (or generate + persist one on first boot), self-verify it, then render the
/// three discovery documents from the PUBLISHED JWKS. The private key is dropped (zeroized) after the
/// self-test — t48 issues no tokens; t49/t50 will retain the handle.
fn build_routes_from_store(store: &OauthKeyStore, issuer: &str) -> Result<OauthRoutes, String> {
    let key = ensure_active_signing_key(store)?;
    self_verify(&key)?;
    tracing::info!(target: "qfs::oauth", kid = %key.kid(), %issuer, "oauth AS signing key ready (ES256); serving PRM + AS-metadata + JWKS (no tokens issued yet)");

    let jwks = published_jwks(store)?;
    build_routes(issuer, &jwks)
}

/// Reload the active signing key, or generate + persist a fresh one if none exists (so a SECOND boot
/// reuses the first boot's key — the binary inserts only when no active key is present). Generation
/// draws 32 bytes of OS entropy; the negligible out-of-range scalar is retried.
fn ensure_active_signing_key(store: &OauthKeyStore) -> Result<SigningKey, String> {
    if let Some(stored) = store
        .active_key()
        .map_err(|e| format!("reading the active oauth key: {e}"))?
    {
        return SigningKey::from_secret_scalar(&stored.private_scalar)
            .map_err(|e| format!("reconstructing the active oauth key: {e}"));
    }

    // First boot: mint a key from OS entropy and persist it (public JWK in the clear, private scalar
    // envelope-sealed). Retry the vanishingly rare out-of-range scalar.
    for _ in 0..8 {
        let mut entropy = [0u8; ENTROPY_BYTES];
        rand::rng().fill_bytes(&mut entropy);
        let Ok(key) = SigningKey::generate(&entropy) else {
            continue;
        };
        let public_jwk = serde_json::to_string(&key.public_jwk())
            .map_err(|e| format!("rendering the oauth public JWK: {e}"))?;
        store
            .insert_active_key(key.kid(), ALG_ES256, &public_jwk, &key.secret_scalar())
            .map_err(|e| format!("persisting the new oauth key: {e}"))?;
        return Ok(key);
    }
    Err("could not generate a valid ES256 signing key from OS entropy".to_string())
}

/// Prove the freshly loaded signing key is operational end-to-end: sign a probe and verify it against
/// the key's own public JWK. This exercises the envelope-decrypt → `Secret` → `SigningKey` path and
/// the JWS primitives t49/t50 depend on. The probe token carries no secret and is discarded (never
/// logged).
fn self_verify(key: &SigningKey) -> Result<(), String> {
    let probe = serde_json::json!({ "kid": key.kid(), "probe": true });
    let token =
        sign_jws(&probe, key).map_err(|e| format!("oauth key self-test sign failed: {e}"))?;
    let jwks = Jwks::new(vec![key.public_jwk()]);
    let claims =
        verify_jws(&token, &jwks).map_err(|e| format!("oauth key self-test verify failed: {e}"))?;
    if claims != probe {
        return Err("oauth key self-test round-trip mismatch".to_string());
    }
    Ok(())
}

/// Parse every published public-JWK JSON string into a [`Jwk`] and collect them into the JWKS body
/// (active first, then any retiring keys during a rotation overlap).
fn published_jwks(store: &OauthKeyStore) -> Result<Jwks, String> {
    let raw = store
        .published_public_jwks()
        .map_err(|e| format!("reading the published JWKS: {e}"))?;
    let mut keys = Vec::with_capacity(raw.len());
    for s in &raw {
        let jwk: Jwk =
            serde_json::from_str(s).map_err(|e| format!("parsing a stored public JWK: {e}"))?;
        keys.push(jwk);
    }
    Ok(Jwks::new(keys))
}

/// Render the three discovery documents for `issuer` + `jwks` into their JSON bodies.
fn build_routes(issuer: &str, jwks: &Jwks) -> Result<OauthRoutes, String> {
    let resource = format!("{issuer}{}", qfs_mcp::MCP_PATH);
    let jwks_uri = format!("{issuer}{JWKS_PATH}");
    let prm = ProtectedResourceMetadata::new(resource, issuer);
    let asm = AuthorizationServerMetadata::new(issuer, jwks_uri);
    Ok(OauthRoutes {
        prm: serde_json::to_vec(&prm).map_err(|e| format!("rendering PRM: {e}"))?,
        asm: serde_json::to_vec(&asm).map_err(|e| format!("rendering AS metadata: {e}"))?,
        jwks: serde_json::to_vec(jwks).map_err(|e| format!("rendering JWKS: {e}"))?,
    })
}

/// Derive the AS issuer (origin URL) from the listener's bind address. `QFS_OAUTH_ISSUER` overrides
/// it for the trusted-reverse-proxy case (decision F: the proxy may terminate TLS and rewrite the
/// host, and the issuer MUST match what the client sees — a constraint t49/t50 must honor). For a
/// loopback bind we advertise `http://localhost:<port>` (mirroring qfs-google-auth's load-bearing
/// `localhost`-not-`127.0.0.1` choice); otherwise `http://<addr>`.
fn issuer_from_addr(addr: SocketAddr) -> String {
    if let Ok(explicit) = std::env::var("QFS_OAUTH_ISSUER") {
        if !explicit.is_empty() {
            return explicit.trim_end_matches('/').to_string();
        }
    }
    if addr.ip().is_loopback() {
        format!("http://localhost:{}", addr.port())
    } else {
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_store::{FileSource, SystemDb};

    fn file_store(path: &std::path::Path) -> OauthKeyStore {
        let conn = SystemDb::open(&FileSource::new(path))
            .unwrap()
            .into_db()
            .into_connection();
        OauthKeyStore::open_or_init(conn, &Secret::from("test-passphrase")).unwrap()
    }

    #[test]
    fn issuer_derivation_prefers_override_then_loopback_localhost() {
        // Loopback bind advertises localhost (not 127.0.0.1).
        let addr: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        assert_eq!(issuer_from_addr(addr), "http://localhost:8787");
        // A non-loopback bind keeps the host:port.
        let public: SocketAddr = "10.0.0.5:9000".parse().unwrap();
        assert_eq!(issuer_from_addr(public), "http://10.0.0.5:9000");
        // The env override wins (decision F: behind a TLS-terminating proxy) and trailing '/' trims.
        std::env::set_var("QFS_OAUTH_ISSUER", "https://qfs.example.com/");
        assert_eq!(issuer_from_addr(addr), "https://qfs.example.com");
        std::env::remove_var("QFS_OAUTH_ISSUER");
    }

    #[test]
    fn second_boot_reuses_the_first_boot_signing_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system.db");
        // First boot: generates + persists an active key.
        let kid1 = {
            let store = file_store(&path);
            ensure_active_signing_key(&store).unwrap().kid().to_string()
        };
        // Second boot (fresh store over the same DB): the SAME active key is reloaded, not a new one.
        let kid2 = {
            let store = file_store(&path);
            ensure_active_signing_key(&store).unwrap().kid().to_string()
        };
        assert_eq!(
            kid1, kid2,
            "the active signing key must be reused on second boot"
        );
    }

    #[test]
    fn boot_built_routes_serve_the_three_documents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system.db");
        let store = file_store(&path);
        let issuer = "http://localhost:8787";
        let routes = build_routes_from_store(&store, issuer).unwrap();

        // PRM points the MCP resource at the issuer as its authorization server.
        let prm: serde_json::Value = serde_json::from_slice(
            &routes
                .handle(&qfs_http::HttpRequest::new(qfs_http::Method::Get, PRM_PATH))
                .unwrap()
                .body,
        )
        .unwrap();
        assert_eq!(prm["resource"], "http://localhost:8787/mcp");
        assert_eq!(prm["authorization_servers"][0], issuer);

        // AS metadata advertises only the live fields (no token/authorization/registration endpoint).
        let asm: serde_json::Value = serde_json::from_slice(
            &routes
                .handle(&qfs_http::HttpRequest::new(qfs_http::Method::Get, ASM_PATH))
                .unwrap()
                .body,
        )
        .unwrap();
        assert_eq!(asm["issuer"], issuer);
        assert_eq!(asm["jwks_uri"], "http://localhost:8787/jwks.json");
        assert!(asm.get("token_endpoint").is_none());

        // JWKS publishes exactly the one active public key, with no private `d` member.
        let jwks: serde_json::Value = serde_json::from_slice(
            &routes
                .handle(&qfs_http::HttpRequest::new(
                    qfs_http::Method::Get,
                    JWKS_PATH,
                ))
                .unwrap()
                .body,
        )
        .unwrap();
        assert_eq!(jwks["keys"].as_array().unwrap().len(), 1);
        assert_eq!(jwks["keys"][0]["use"], "sig");
        assert_eq!(jwks["keys"][0]["alg"], "ES256");
        assert!(
            jwks["keys"][0].get("d").is_none(),
            "no private scalar in JWKS"
        );
    }

    #[test]
    fn non_get_and_unknown_paths_fall_through() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system.db");
        let routes = build_routes_from_store(&file_store(&path), "http://localhost:8787").unwrap();
        // A POST to a well-known path falls through (read-only discovery).
        assert!(routes
            .handle(&qfs_http::HttpRequest::new(
                qfs_http::Method::Post,
                JWKS_PATH
            ))
            .is_none());
        // An unrelated path falls through.
        assert!(routes
            .handle(&qfs_http::HttpRequest::new(
                qfs_http::Method::Get,
                "/status"
            ))
            .is_none());
    }
}
