//! The two discovery documents: [`ProtectedResourceMetadata`] (RFC 9728) and
//! [`AuthorizationServerMetadata`] (RFC 8414).
//!
//! ## Honesty (the t48 sequencing rule, option (a))
//! [`AuthorizationServerMetadata`] advertises ONLY what is live this milestone: `issuer`,
//! `jwks_uri`, and the static `response_types_supported` / `code_challenge_methods_supported`
//! (`["S256"]`). The `authorization_endpoint` / `token_endpoint` / `registration_endpoint` members
//! are `Option`s that stay `None` (and are OMITTED from the JSON via `skip_serializing_if`) until
//! **t49** actually serves those endpoints — advertising an endpoint that 404s would mislead a
//! discovering client. t49 sets the fields; the document shape does not otherwise change.

use serde::{Deserialize, Serialize};

/// **RFC 9728** Protected Resource Metadata — served at
/// `/.well-known/oauth-protected-resource`. It tells a client which authorization server(s) guard a
/// protected resource (here, the qfs MCP endpoint), so the client knows where to obtain a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedResourceMetadata {
    /// The protected resource's identifier — the absolute URL of the qfs MCP endpoint.
    pub resource: String,
    /// The authorization server issuer identifier(s) that can issue tokens for `resource`.
    pub authorization_servers: Vec<String>,
    /// How a bearer token is presented to the resource — `header` (the `Authorization` header).
    /// Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bearer_methods_supported: Vec<String>,
}

impl ProtectedResourceMetadata {
    /// Build the PRM for the MCP `resource` guarded by the single authorization-server `issuer`.
    /// Advertises bearer presentation via the `Authorization` header (the MCP transport convention).
    #[must_use]
    pub fn new(resource: impl Into<String>, issuer: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            authorization_servers: vec![issuer.into()],
            bearer_methods_supported: vec!["header".to_string()],
        }
    }
}

/// **RFC 8414** Authorization Server Metadata — served at
/// `/.well-known/oauth-authorization-server`. See the module docs for the t48 honesty rule: only
/// `issuer`, `jwks_uri`, and the static capability arrays are advertised; the endpoint fields stay
/// `None`/omitted until t49.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationServerMetadata {
    /// The authorization server's issuer identifier (its base origin URL). MUST match what the
    /// client used to fetch this document (RFC 8414 §3.3).
    pub issuer: String,
    /// The absolute URL of this AS's JWK Set (`/jwks.json`) — live this milestone.
    pub jwks_uri: String,
    /// The OAuth `response_type` values supported — `["code"]` (the auth-code flow t49 will serve).
    pub response_types_supported: Vec<String>,
    /// The PKCE code-challenge methods supported — `["S256"]` (RFD §10: PKCE is mandatory).
    pub code_challenge_methods_supported: Vec<String>,
    /// The OAuth grant types supported. Empty/omitted at t48; t49 sets `authorization_code` (+
    /// `refresh_token`) when the token endpoint goes live.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grant_types_supported: Vec<String>,
    /// The authorization endpoint — **None/omitted until t49** (advertising a dead endpoint would
    /// mislead a client; see the module docs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
    /// The token endpoint — **None/omitted until t49**.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    /// The dynamic-client-registration endpoint — **None/omitted until t49**.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,
}

impl AuthorizationServerMetadata {
    /// Build the t48 AS metadata for `issuer`, publishing only the live `jwks_uri` + the static
    /// capability arrays. The endpoint fields are `None` (omitted) until t49 serves them.
    #[must_use]
    pub fn new(issuer: impl Into<String>, jwks_uri: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            jwks_uri: jwks_uri.into(),
            response_types_supported: vec!["code".to_string()],
            code_challenge_methods_supported: vec!["S256".to_string()],
            grant_types_supported: Vec::new(),
            authorization_endpoint: None,
            token_endpoint: None,
            registration_endpoint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUER: &str = "http://localhost:8787";

    #[test]
    fn prm_golden_shape_points_at_the_issuer() {
        let prm = ProtectedResourceMetadata::new(format!("{ISSUER}/mcp"), ISSUER);
        let v = serde_json::to_value(&prm).unwrap();
        assert_eq!(v["resource"], "http://localhost:8787/mcp");
        assert_eq!(v["authorization_servers"][0], ISSUER);
        assert_eq!(v["bearer_methods_supported"][0], "header");
        // Exactly the three members.
        assert_eq!(v.as_object().unwrap().len(), 3);
    }

    #[test]
    fn as_metadata_golden_shape_advertises_only_live_fields() {
        let asm = AuthorizationServerMetadata::new(ISSUER, format!("{ISSUER}/jwks.json"));
        let v = serde_json::to_value(&asm).unwrap();
        assert_eq!(v["issuer"], ISSUER);
        assert_eq!(v["jwks_uri"], "http://localhost:8787/jwks.json");
        assert_eq!(v["response_types_supported"][0], "code");
        assert_eq!(v["code_challenge_methods_supported"][0], "S256");
        // HONESTY (t48 option a): the endpoints t49 will serve are NOT advertised yet — a client
        // must not see a token/authorization/registration endpoint that does not exist.
        assert!(v.get("token_endpoint").is_none());
        assert!(v.get("authorization_endpoint").is_none());
        assert!(v.get("registration_endpoint").is_none());
        assert!(v.get("grant_types_supported").is_none());
        // Exactly the four live members.
        assert_eq!(v.as_object().unwrap().len(), 4);
    }

    #[test]
    fn as_metadata_round_trips_and_t49_can_set_endpoints() {
        let mut asm = AuthorizationServerMetadata::new(ISSUER, format!("{ISSUER}/jwks.json"));
        // The t49 seam: setting the endpoint makes it appear in the document.
        asm.token_endpoint = Some(format!("{ISSUER}/oauth/token"));
        let v = serde_json::to_value(&asm).unwrap();
        assert_eq!(v["token_endpoint"], "http://localhost:8787/oauth/token");
        let back: AuthorizationServerMetadata = serde_json::from_value(v).unwrap();
        assert_eq!(asm, back);
    }
}
