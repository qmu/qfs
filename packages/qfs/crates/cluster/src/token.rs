//! Join tokens: `base64url(payload) "." base64url(HMAC-SHA256(secret, payload))`.
//!
//! The payload is JSON `{member, exp, nonce}`. The host alone holds the secret, so only the host
//! can mint a token, and any edit to the payload breaks the signature. A token names exactly one
//! member and expires; the host refuses a bad signature, an expired token, or a name mismatch.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// The signed claims a join token carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClaims {
    /// The member name this token admits.
    pub member: String,
    /// Expiry, seconds since the Unix epoch.
    pub exp: u64,
    /// Random nonce so two tokens for the same member differ.
    pub nonce: String,
}

/// Why a token was refused. The messages never echo the token or the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// Not `payload.signature`, bad base64, or bad JSON.
    Malformed,
    /// The signature does not verify under the host secret (tampered or foreign).
    BadSignature,
    /// The token is past its expiry.
    Expired,
    /// The token was minted for a different member name.
    NameMismatch {
        /// The name the token admits.
        token: String,
        /// The name the member presented.
        presented: String,
    },
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => write!(f, "the join token is malformed"),
            Self::BadSignature => write!(f, "the join token's signature does not verify"),
            Self::Expired => write!(f, "the join token has expired"),
            Self::NameMismatch { token, presented } => write!(
                f,
                "the join token admits member `{token}`, not `{presented}`"
            ),
        }
    }
}

impl std::error::Error for TokenError {}

/// A fresh random 32-byte cluster secret.
#[must_use]
pub fn new_secret() -> [u8; 32] {
    let mut s = [0u8; 32];
    rand::rng().fill_bytes(&mut s);
    s
}

fn sign(secret: &[u8], payload: &[u8]) -> Vec<u8> {
    // HMAC accepts any key length, so `new_from_slice` cannot fail; fall back to an empty tag
    // (which never verifies) rather than panic.
    match HmacSha256::new_from_slice(secret) {
        Ok(mut mac) => {
            mac.update(payload);
            mac.finalize().into_bytes().to_vec()
        }
        Err(_) => Vec::new(),
    }
}

/// Mint a token for `member` valid for `ttl_secs` from `now`.
#[must_use]
pub fn mint(secret: &[u8], member: &str, ttl_secs: u64, now: u64) -> String {
    let mut nonce = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce);
    let claims = TokenClaims {
        member: member.to_string(),
        exp: now.saturating_add(ttl_secs),
        nonce: URL_SAFE_NO_PAD.encode(nonce),
    };
    let payload = serde_json::to_vec(&claims).unwrap_or_default();
    let sig = sign(secret, &payload);
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(sig)
    )
}

/// Verify `token` under `secret` at `now`. With `presented` set, the token must admit that name.
///
/// # Errors
/// A [`TokenError`] naming the refusal.
pub fn verify(
    secret: &[u8],
    token: &str,
    presented: Option<&str>,
    now: u64,
) -> Result<TokenClaims, TokenError> {
    let (p64, s64) = token.trim().split_once('.').ok_or(TokenError::Malformed)?;
    let payload = URL_SAFE_NO_PAD
        .decode(p64)
        .map_err(|_| TokenError::Malformed)?;
    let sig = URL_SAFE_NO_PAD
        .decode(s64)
        .map_err(|_| TokenError::Malformed)?;
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| TokenError::BadSignature)?;
    mac.update(&payload);
    // Constant-time comparison.
    mac.verify_slice(&sig)
        .map_err(|_| TokenError::BadSignature)?;
    let claims: TokenClaims =
        serde_json::from_slice(&payload).map_err(|_| TokenError::Malformed)?;
    if now >= claims.exp {
        return Err(TokenError::Expired);
    }
    if let Some(name) = presented {
        if name != claims.member {
            return Err(TokenError::NameMismatch {
                token: claims.member,
                presented: name.to_string(),
            });
        }
    }
    Ok(claims)
}

/// Parse a TTL like `24h`, `30m`, `90s`, `7d` or a bare number of seconds.
///
/// # Errors
/// A message naming the unparseable input.
pub fn parse_ttl(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 3600),
        Some('d') => (&s[..s.len() - 1], 86_400),
        _ => (s, 1),
    };
    num.parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| n.saturating_mul(mult))
        .ok_or_else(|| format!("`{s}` is not a TTL (use e.g. 24h, 30m, 7d)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn a_minted_token_verifies_for_its_member() {
        let t = mint(SECRET, "alice", 60, 1000);
        let c = verify(SECRET, &t, Some("alice"), 1001).unwrap();
        assert_eq!(c.member, "alice");
        assert_eq!(c.exp, 1060);
        assert_eq!(verify(SECRET, &t, None, 1001).unwrap().member, "alice");
    }

    #[test]
    fn an_expired_token_is_refused() {
        let t = mint(SECRET, "alice", 60, 1000);
        assert_eq!(
            verify(SECRET, &t, Some("alice"), 1060),
            Err(TokenError::Expired)
        );
    }

    #[test]
    fn a_tampered_payload_is_refused() {
        let t = mint(SECRET, "alice", 60, 1000);
        let (_, sig) = t.split_once('.').unwrap();
        let forged = URL_SAFE_NO_PAD.encode(br#"{"member":"alice","exp":99999999999,"nonce":"x"}"#);
        assert_eq!(
            verify(SECRET, &format!("{forged}.{sig}"), Some("alice"), 1001),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn a_foreign_secret_is_refused() {
        let t = mint(b"another-secret-another-secret-xx", "alice", 60, 1000);
        assert_eq!(
            verify(SECRET, &t, Some("alice"), 1001),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn a_name_mismatch_is_refused() {
        let t = mint(SECRET, "alice", 60, 1000);
        assert!(matches!(
            verify(SECRET, &t, Some("mallory"), 1001),
            Err(TokenError::NameMismatch { .. })
        ));
    }

    #[test]
    fn garbage_is_malformed() {
        assert_eq!(verify(SECRET, "nope", None, 0), Err(TokenError::Malformed));
        assert_eq!(verify(SECRET, "a.b!", None, 0), Err(TokenError::Malformed));
    }

    #[test]
    fn ttl_parses() {
        assert_eq!(parse_ttl("24h"), Ok(86_400));
        assert_eq!(parse_ttl("30m"), Ok(1800));
        assert_eq!(parse_ttl("7d"), Ok(604_800));
        assert_eq!(parse_ttl("90"), Ok(90));
        assert!(parse_ttl("0h").is_err());
        assert!(parse_ttl("soon").is_err());
    }
}
