---
created_at: 2026-07-04T15:26:40+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, UX]
effort:
commit_hash: c41679e
category: Added
depends_on:
---

# Console bundle delivery: fetch → verify → cache → self-serve, with pinned pairing

## Overview

Implement blueprint §14's delivery model for the qfs console (the plgg plug-based SPA): the
UI is **loaded, not embedded**, and the browser never touches a third-party origin.

- **Pinned pairing**: each server release carries a coordinate — the paired UI bundle's source
  URL + integrity hash (bytes in the binary: a URL and a hash, not the UI). The release
  pipeline (§12) gains the step that stamps it.
- **Fetch → verify → cache**: on boot / first console access, the server downloads the bundle,
  verifies the hash, and stores it in the local state dir; a tampered or mismatched bundle is
  refused with a structured error (the console then simply isn't served — qfs itself is
  unaffected).
- **Self-serve**: the browser gets the bundle only from the local qfs server (same-origin CSP;
  offline after first fetch). Version skew is structurally absent — the server serves exactly
  what it pinned.
- **Override**: a `QFS_UI_URL`-style source override for development against a live plgg dev
  server and for self-hosted mirrors (vendor neutrality; the default points at the official
  deploy). An override skips the pin (dev mode is explicit, logged, and off by default).
- The plgg console application itself is the plgg repository's work; this ticket is the qfs
  side only (delivery, pairing, serving, and the security posture). The existing embedded
  approval-cards dashboard stays until the console reaches parity (§14).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions
- `workaholic:design` / `policies/defense-in-depth.md` — hash verification, same-origin serving, and the explicit dev override are independent layers; the browser never trusts a delivery edge
- `workaholic:design` / `policies/vendor-neutrality.md` — the bundle source is overridable; no hard dependency on one CDN
- `workaholic:implementation` / `policies/observability.md` — fetch/verify outcomes are logged structurally; a refused bundle names why (hash mismatch vs unreachable), secret-free
- `workaholic:operation` / `policies/ci-cd.md` — the pairing stamp is a release-pipeline step, reproducible locally

## Key Files

- `packages/qfs/crates/qfs/src/dashboard.rs` + `crates/qfs/src/serve.rs` - the current embedded-SPA serving path the console route joins (and eventually replaces)
- `packages/qfs/crates/http/src/` - the serving face (routes, CSP headers)
- `packages/qfs/crates/host/src/` - the state dir where the verified bundle caches
- `.github/workflows/release.yml` + `packages/qfs/xtask/` - the release step that stamps the pairing coordinate
- `docs/blueprint.md` §14 - the authority

## Implementation Steps

1. Define the pairing coordinate (URL + sha256) and where it lives in the binary; add the
   release-pipeline stamping step.
2. Implement fetch-verify-cache into the state dir (write-temp → verify → rename; refusal is
   structured and non-fatal to the rest of qfs).
3. Serve the cached bundle from the local server under the console route with same-origin CSP;
   no runtime third-party origin.
4. Implement the explicit dev/self-host override (skips the pin, logged, off by default).
5. Hermetic tests: hash-mismatch refusal; offline-after-cache serving; override path; CSP
   headers present; qfs healthy with no bundle available.

## Quality Gate

**Acceptance criteria:**

- A correct bundle is fetched, verified, cached, and served locally (hermetic, MockHttp-style
  fixture); a tampered bundle is refused with a structured, secret-free error and qfs keeps
  serving everything else.
- The browser-facing responses carry same-origin CSP; no runtime reference to a third-party
  origin exists in the served page.
- The dev override works and is explicitly logged; without it, only the pinned bundle is ever
  served.
- The release pipeline stamps the pairing coordinate reproducibly.

**Verification method:** `cargo test --workspace`; `clippy --workspace --all-targets -- -D
warnings`; `fmt --all --check`; `gen-docs --check`; `gen-skills --check`.

**Gate:** all green including the named tests; the security posture (verify + same-origin +
explicit override) demonstrated by tests, not assertion.

## Considerations

- The fetch happens at most once per pin — no auto-update loop; updating the UI means a new
  server release (the signed-channel independent-release model is a §14 named park)
- Refusal must never brick qfs: the console is optional; the engine, CLI, MCP, and existing
  dashboard keep working with no bundle
- The bundle cache lives in the state dir (`QFS_STATE_DIR` convention), never a system path
- qfs is experimental: the pairing/cache format may hard-break freely
