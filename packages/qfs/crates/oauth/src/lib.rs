//! `qfs-oauth` — the **OAuth 2.1 authorization-server domain leaf** (roadmap **M2**, ticket t48).
//!
//! This crate is the *discovery + key-publication* half of making a qfs server its **own**
//! authorization server (RFD-0001 decision **C**, §4.1 — authorization kept separate from the t45
//! identity store). It builds the three public, read-only documents a remote-MCP client discovers
//! during the handshake, plus the signing-key machinery behind them:
//!
//! - [`ProtectedResourceMetadata`] — **RFC 9728** Protected Resource Metadata: the MCP endpoint
//!   (`resource`) points a client at the authorization server(s) that guard it.
//! - [`AuthorizationServerMetadata`] — **RFC 8414** Authorization Server Metadata. Per the t48
//!   honesty rule (option (a)), this advertises ONLY what is live this milestone: `issuer`,
//!   `jwks_uri`, and the static `response_types_supported` / `code_challenge_methods_supported`
//!   (`["S256"]`). The `token_endpoint` / `authorization_endpoint` / `registration_endpoint`
//!   fields exist as `Option`s that stay `None` (omitted from the JSON) until **t49** serves them —
//!   advertising a dead endpoint would mislead a client.
//! - [`Jwks`] — the JSON Web Key Set: each active/retiring public key rendered as a [`Jwk`]
//!   (`kty=EC` / `crv=P-256` / `use=sig` / `alg=ES256` / `kid`). Multiple keys are publishable so a
//!   future rotation can overlap an `active` and a `retiring` key (the rotation *trigger* is a
//!   documented seam, not implemented here).
//!
//! ## JWS signing — ES256 over vetted pure-Rust ECDSA
//! [`sign_jws`] / [`verify_jws`] are the thin JWS compact-serialization primitives **t49/t50** will
//! consume to mint/verify access tokens. They are built over the RustCrypto [`p256`] ECDSA
//! implementation (ES256 = NIST P-256 + SHA-256) plus [`qfs_crypto_core`]'s SHA-256 for the RFC 7638
//! `kid` thumbprint — **never** a hand-rolled curve, and **never** a heavy JWT SDK. t48 issues no
//! tokens; these are the primitives, golden-vector tested.
//!
//! ## Secret discipline (RFD §10)
//! The AS PRIVATE signing key is the crown jewel. In this crate it is carried ONLY inside the
//! redacting, zeroized [`qfs_secrets::Secret`] (see [`SigningKey::from_secret_scalar`] /
//! [`SigningKey::secret_scalar`]) — never a bare `String`/`Vec<u8>` — and it is **never**
//! serialized: every document this crate produces ([`Jwk`] included) carries only PUBLIC key
//! material. At-rest the key is envelope-encrypted by the binary-injected `qfs-store` layer; this
//! pure leaf only signs/verifies and renders public documents.
//!
//! ## Topology
//! A pure-ish leaf: it depends on the zero-dep crypto leaf, the `Secret` wrapper, `p256`, and serde
//! only — **no** tokio (the async HTTP binding that serves the routes lives in the terminal binary)
//! and **no** rusqlite (key persistence is the binary-injected `qfs-store` layer). The dep-direction
//! guard `oauth_is_a_pure_domain_leaf` pins this.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod jwks;
mod key;
mod metadata;
mod sign;

pub use jwks::{Jwk, Jwks};
pub use key::{OauthError, SigningKey};
pub use metadata::{AuthorizationServerMetadata, ProtectedResourceMetadata};
pub use sign::{sign_jws, verify_jws, Claims};

/// The fixed JWS/JWK algorithm this AS signs with: **ES256** (ECDSA using NIST P-256 + SHA-256).
/// A single, vetted choice (decision C: smaller keys + simpler encoding than RS256, pure-Rust
/// `p256`). The `kid` selects the concrete key; the `alg` is constant.
pub const ALG_ES256: &str = "ES256";
