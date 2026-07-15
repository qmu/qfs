//! Console bundle delivery (blueprint §14): **fetch → verify → cache → self-serve**.
//!
//! The qfs console is a plgg plug-based SPA that is *loaded, not embedded*. The browser never
//! touches a third-party origin: the server fetches the release-pinned bundle once, verifies its
//! integrity hash, caches it locally, and serves it **same-origin**. This screen operates a
//! credential-holding control plane, so a compromised delivery edge must not be able to own it —
//! a tampered bundle fails hash verification and is refused, and the console simply is not served
//! (the engine, CLI, MCP, and embedded dashboard keep working, unaffected). Version pairing is
//! pinned by the *server*, so server/client skew is structurally absent.
//!
//! This module is the qfs delivery side only (pairing, fetch, verify, cache, serve, override) —
//! the plgg console application itself lives in the plgg repository. It is credential-free and
//! **network-free in tests**: fetching is a [`BundleFetcher`] seam the binary wires to a real HTTP
//! client and tests inject a mock, and every decision below the fetch is a pure function.

use std::path::{Path, PathBuf};

/// The pinned pairing coordinate: the paired UI bundle's source URL + its lowercase-hex sha256
/// integrity hash. Bytes in the binary — a *coordinate*, not the UI.
#[derive(Debug, Clone, Copy)]
pub struct PairingCoordinate {
    /// The source URL the bundle is fetched from (once, at boot / first console access).
    pub url: &'static str,
    /// The lowercase-hex sha256 the fetched bytes MUST hash to, or the fetch is refused.
    pub sha256_hex: &'static str,
}

/// The bundle this server release is paired with. **Unset** (empty `sha256_hex`) until the plgg
/// console publishes its first paired bundle: an unset pin means "no console to serve" — honest and
/// non-fatal. The release pipeline (§12) stamps the real URL + hash here at pairing time.
pub const PINNED_BUNDLE: PairingCoordinate = PairingCoordinate {
    url: "",
    sha256_hex: "",
};

/// The env var naming a bundle source override (dev against a live plgg dev server / a self-hosted
/// mirror). Set = **dev mode**: the pin is skipped (explicit, logged, off by default).
pub const UI_URL_ENV: &str = "QFS_UI_URL";

/// The same-origin Content-Security-Policy the served console page carries: every resource class is
/// `'self'` only, no third-party origin at runtime — so even a bundle that *tried* to reach another
/// origin is blocked by the browser (defense in depth atop the fetch-time hash verification).
pub const CONSOLE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'self'; frame-ancestors 'none'";

/// A structured, secret-free reason a bundle was refused — never fatal to the rest of qfs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleError {
    /// No bundle is pinned for this release and no override was set — nothing to serve.
    NoPin,
    /// The bundle source could not be fetched (network / URL). Carries a secret-free reason.
    Unreachable(String),
    /// The fetched bytes did not hash to the pinned value — a tampered / mismatched delivery.
    HashMismatch {
        /// The pinned (expected) lowercase-hex sha256.
        expected: String,
        /// The fetched bytes' actual lowercase-hex sha256.
        got: String,
    },
    /// The verified bytes could not be cached (I/O). Carries a secret-free reason.
    Cache(String),
}

impl std::fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsoleError::NoPin => write!(f, "no console bundle is pinned for this release"),
            ConsoleError::Unreachable(why) => write!(f, "console bundle unreachable: {why}"),
            ConsoleError::HashMismatch { expected, got } => write!(
                f,
                "console bundle refused: integrity hash mismatch (expected {expected}, got {got})"
            ),
            ConsoleError::Cache(why) => write!(f, "console bundle could not be cached: {why}"),
        }
    }
}

/// A byte-fetcher seam: the binary wires a real HTTP client; tests inject a mock (NO network in
/// this module or its tests). One method — URL → bytes, or a secret-free unreachable reason.
pub trait BundleFetcher {
    /// Fetch the bytes at `url`, or a secret-free reason it was unreachable.
    ///
    /// # Errors
    /// A short, secret-free description of why the fetch failed.
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// The resolved delivery decision — pure, from the pinned coordinate + an optional override. Kept
/// separate from any env read so it is testable without touching the process environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Serve the pinned bundle, verifying the fetched bytes against `sha256_hex`.
    Pinned {
        /// The pinned source URL.
        url: String,
        /// The pinned lowercase-hex sha256 to verify against.
        sha256_hex: String,
    },
    /// **Dev mode**: fetch from the override URL and SKIP the pin (used only deliberately, logged).
    Dev {
        /// The override source URL.
        url: String,
    },
    /// No pin and no override — there is no console to serve.
    None,
}

/// Resolve the delivery mode: an override (dev) wins over the pin; an empty pin with no override is
/// [`Delivery::None`]. Pure — the binary passes [`source_override`]'s value in.
#[must_use]
pub fn resolve_delivery(coord: &PairingCoordinate, override_url: Option<&str>) -> Delivery {
    if let Some(url) = override_url.filter(|s| !s.is_empty()) {
        return Delivery::Dev {
            url: url.to_string(),
        };
    }
    if coord.sha256_hex.is_empty() {
        return Delivery::None;
    }
    Delivery::Pinned {
        url: coord.url.to_string(),
        sha256_hex: coord.sha256_hex.to_string(),
    }
}

/// The active source override (`QFS_UI_URL`), or `None` for the pinned default. Reading the process
/// env is confined to this one function so the rest of the module stays pure/testable.
#[must_use]
pub fn source_override() -> Option<String> {
    std::env::var(UI_URL_ENV).ok().filter(|s| !s.is_empty())
}

/// The cache path for a verified bundle under a state dir (`<state_dir>/console/bundle`).
#[must_use]
pub fn cache_path(state_dir: &Path) -> PathBuf {
    state_dir.join("console").join("bundle")
}

/// Fetch → verify → cache for a resolved [`Delivery`]. A [`Delivery::Pinned`] verifies the fetched
/// bytes against the pinned hash and refuses a mismatch; a [`Delivery::Dev`] skips verification
/// (the caller logs that an unverified source is in use); [`Delivery::None`] is [`ConsoleError::NoPin`].
/// The verified bytes are cached atomically (write a temp sibling, then rename into place). Every
/// failure is a structured [`ConsoleError`] the caller logs — the console is simply not served.
///
/// # Errors
/// [`ConsoleError`] — no pin, unreachable source, hash mismatch, or a cache I/O failure.
pub fn deliver(
    delivery: &Delivery,
    fetcher: &dyn BundleFetcher,
    state_dir: &Path,
) -> Result<PathBuf, ConsoleError> {
    let (url, verify) = match delivery {
        Delivery::None => return Err(ConsoleError::NoPin),
        Delivery::Pinned { url, sha256_hex } => (url.as_str(), Some(sha256_hex.as_str())),
        Delivery::Dev { url } => (url.as_str(), None),
    };

    let bytes = fetcher.fetch(url).map_err(ConsoleError::Unreachable)?;
    if let Some(expected) = verify {
        let got = qfs_crypto_core::sha256_hex(&bytes);
        if got != expected {
            return Err(ConsoleError::HashMismatch {
                expected: expected.to_string(),
                got,
            });
        }
    }
    cache_bytes(state_dir, &bytes)
}

/// Cache already-verified bytes atomically: write a temp sibling, then rename into the cache path
/// (an atomic replace on the same filesystem, so a concurrent read never sees a half-written file).
fn cache_bytes(state_dir: &Path, bytes: &[u8]) -> Result<PathBuf, ConsoleError> {
    let dest = cache_path(state_dir);
    let Some(dir) = dest.parent() else {
        return Err(ConsoleError::Cache("cache path has no parent".to_string()));
    };
    std::fs::create_dir_all(dir).map_err(|e| ConsoleError::Cache(e.kind().to_string()))?;
    let tmp = dir.join("bundle.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| ConsoleError::Cache(e.kind().to_string()))?;
    std::fs::rename(&tmp, &dest).map_err(|e| ConsoleError::Cache(e.kind().to_string()))?;
    Ok(dest)
}

/// Read the cached bundle bytes if present (offline after the first fetch). `None` when no bundle
/// has been cached — the console is not served and the engine/CLI/MCP/dashboard are unaffected.
#[must_use]
pub fn read_cached(state_dir: &Path) -> Option<Vec<u8>> {
    std::fs::read(cache_path(state_dir)).ok()
}

/// The served console page: the cached bytes plus the security headers the local server attaches
/// (same-origin CSP). Owned, transport-free data so the binary maps it onto its own HTTP response
/// type and this module (and its tests) stay network-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedConsole {
    /// The `Content-Type` for the console entry page.
    pub content_type: &'static str,
    /// The same-origin `Content-Security-Policy` ([`CONSOLE_CSP`]).
    pub csp: &'static str,
    /// The cached bundle bytes, served only by the local qfs server (never a third-party origin).
    pub body: Vec<u8>,
}

/// Build the console page response from the local cache: the cached bundle bytes + the same-origin
/// security headers. `None` when no bundle is cached, so the console route 404s and the rest of qfs
/// is unaffected. **Same-origin only** — the browser is served by the local qfs server, never a
/// delivery edge.
#[must_use]
pub fn serve(state_dir: &Path) -> Option<ServedConsole> {
    read_cached(state_dir).map(|body| ServedConsole {
        content_type: "text/html; charset=utf-8",
        csp: CONSOLE_CSP,
        body,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// A mock fetcher returning fixed bytes (or an unreachable error) — no network.
    struct MockFetcher(Result<Vec<u8>, String>);
    impl BundleFetcher for MockFetcher {
        fn fetch(&self, _url: &str) -> Result<Vec<u8>, String> {
            self.0.clone()
        }
    }

    fn tmp_state() -> PathBuf {
        // A unique-per-call dir under the build TMPDIR (set by the harness); never a system path.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let base = std::env::var("TMPDIR").unwrap_or_else(|_| ".".to_string());
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(base).join(format!("qfs-console-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn correct_bundle_is_fetched_verified_cached_and_served() {
        let bytes = b"<!doctype html><title>qfs console</title>".to_vec();
        let hash = qfs_crypto_core::sha256_hex(&bytes);
        let delivery = Delivery::Pinned {
            url: "https://example/bundle".to_string(),
            sha256_hex: hash,
        };
        let state = tmp_state();
        let fetcher = MockFetcher(Ok(bytes.clone()));

        let path = deliver(&delivery, &fetcher, &state).expect("verified + cached");
        assert_eq!(path, cache_path(&state));
        // Offline after cache: reading the cache returns exactly the fetched bytes.
        assert_eq!(read_cached(&state), Some(bytes));
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn tampered_bundle_is_refused_and_nothing_is_cached() {
        let delivery = Delivery::Pinned {
            url: "https://example/bundle".to_string(),
            // A hash that the tampered bytes will NOT match.
            sha256_hex: qfs_crypto_core::sha256_hex(b"the original, untampered bundle"),
        };
        let state = tmp_state();
        let fetcher = MockFetcher(Ok(b"a tampered bundle from a compromised edge".to_vec()));

        match deliver(&delivery, &fetcher, &state) {
            Err(ConsoleError::HashMismatch { .. }) => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        // The refusal cached nothing — the console is simply not served.
        assert!(read_cached(&state).is_none());
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn unreachable_source_is_structured_not_fatal() {
        let delivery = Delivery::Pinned {
            url: "https://example/bundle".to_string(),
            sha256_hex: qfs_crypto_core::sha256_hex(b"x"),
        };
        let state = tmp_state();
        let fetcher = MockFetcher(Err("connection refused".to_string()));
        match deliver(&delivery, &fetcher, &state) {
            Err(ConsoleError::Unreachable(why)) => assert!(why.contains("refused")),
            other => panic!("expected Unreachable, got {other:?}"),
        }
        assert!(read_cached(&state).is_none());
    }

    #[test]
    fn dev_override_skips_the_pin() {
        // An override resolves to Dev (no verification) and wins over a pin.
        let coord = PairingCoordinate {
            url: "https://official/bundle",
            sha256_hex: "abc123",
        };
        assert_eq!(
            resolve_delivery(&coord, Some("http://localhost:5173")),
            Delivery::Dev {
                url: "http://localhost:5173".to_string()
            }
        );
        // Dev mode fetches and caches WITHOUT a hash check (any bytes are accepted, deliberately).
        let state = tmp_state();
        let fetcher = MockFetcher(Ok(b"a live dev bundle, unhashed".to_vec()));
        let dev = Delivery::Dev {
            url: "http://localhost:5173".to_string(),
        };
        assert!(deliver(&dev, &fetcher, &state).is_ok());
        assert!(read_cached(&state).is_some());
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn no_pin_and_no_override_serves_no_console() {
        // The default release state today: empty pin, no override → None → NoPin (non-fatal).
        assert_eq!(resolve_delivery(&PINNED_BUNDLE, None), Delivery::None);
        let state = tmp_state();
        let fetcher = MockFetcher(Ok(b"unused".to_vec()));
        assert_eq!(
            deliver(&Delivery::None, &fetcher, &state),
            Err(ConsoleError::NoPin)
        );
        assert!(read_cached(&state).is_none());
    }

    #[test]
    fn served_page_carries_the_bundle_and_same_origin_csp() {
        let bytes = b"<!doctype html><title>qfs console</title>".to_vec();
        let hash = qfs_crypto_core::sha256_hex(&bytes);
        let state = tmp_state();
        let fetcher = MockFetcher(Ok(bytes.clone()));
        let delivery = Delivery::Pinned {
            url: "https://example/bundle".to_string(),
            sha256_hex: hash,
        };
        deliver(&delivery, &fetcher, &state).expect("cached");

        let served = serve(&state).expect("a cached bundle is served");
        assert_eq!(served.body, bytes);
        assert_eq!(served.csp, CONSOLE_CSP);
        assert!(served.content_type.contains("text/html"));

        // No cache → no console served (the route 404s; qfs otherwise unaffected).
        let empty = tmp_state();
        assert!(serve(&empty).is_none());
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn csp_is_same_origin_only() {
        // No third-party origin appears in the policy; every class is 'self'.
        assert!(CONSOLE_CSP.contains("default-src 'self'"));
        assert!(CONSOLE_CSP.contains("connect-src 'self'"));
        assert!(!CONSOLE_CSP.contains("http://"));
        assert!(!CONSOLE_CSP.contains("https://"));
    }
}
