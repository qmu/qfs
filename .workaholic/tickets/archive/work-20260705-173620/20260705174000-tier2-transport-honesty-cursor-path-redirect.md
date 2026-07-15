---
created_at: 2026-07-05T17:40:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 4h
commit_hash: 1509c9d
category: Added
depends_on: []
---

# §13 tier-2 (1/3): transport honesty — dotted cursor path + redirect confinement

## Overview

Two transport-level generalizations from the blueprint §13 **Tier 2** decision (see the new
"Tier 2 — a declared view IS its stored query" bullet), both independent of body evaluation:

1. **Dotted cursor path** (parity gap ②): `Pagination::Cursor`'s `next_field` becomes a dotted
   **path** — `PAGINATE CURSOR (next 'response_metadata.next_cursor' param 'cursor' MAX 50)` —
   and `cursor_from_body` (driver-http `applier.rs:331`) walks it (split on `.`, descend JSON
   objects). A plain field name is the 1-segment case, so every existing config keeps working
   unchanged (not that we owe compat — it just falls out).
2. **Redirect confinement** (the recorded §13 security park; concern
   `21-tier-1-declared-driver-scope-stops`): reqwest follows 30x internally
   (`driver-http/src/client.rs`), so the `send_one` chokepoint never sees a redirect target.
   Give `ReqwestClient` a constructor that pins a **redirect policy to a set of allowed hosts**
   (custom `redirect::Policy`: follow only when the target host is in the set; otherwise stop
   and surface a structured `HttpError::Confinement`), and make `declared_http_client`
   (qfs `declared_driver.rs:567`) build the declared driver's client with its confined host.
   The compiled `/rest` path keeps the default client; this is the declared-driver boundary.

## Key files

- `packages/qfs/crates/driver-http/src/applier.rs` — `cursor_from_body` path walk
- `packages/qfs/crates/driver-http/src/client.rs` — `ReqwestClient` redirect-policy constructor
- `packages/qfs/crates/driver-http/src/error.rs` — reuse `HttpError::Confinement`
- `packages/qfs/crates/qfs/src/declared_driver.rs` — `declared_http_client` takes the host

## Implementation steps

1. `cursor_from_body`: walk `next_field` as a dotted path over the JSON body; add unit tests
   (top-level field, nested `response_metadata.next_cursor`, missing path → None).
2. `ReqwestClient::with_confined_hosts(hosts)` building a `redirect::Policy::custom` that
   compares each hop's host against the set; a refused hop errors structurally (secret-free).
   Unit-test the policy decision function directly (extract it pure so it tests without a
   socket).
3. Thread the confined host into `declared_http_client(&DeclaredDriver)`; the shipped MockHttp
   test path is unaffected (mock client has no redirects).
4. Bump the patch version (0.0.22 → 0.0.23) — first shipped ticket of this branch.
5. Gates: `cargo test --workspace`, clippy `-D warnings`, `fmt --check` (unpiped), gen-docs,
   gen-skills — sequential.

## Quality gate

- A nested cursor (`response_metadata.next_cursor`) drives the bounded follow loop in a
  hermetic pagination test (two MockHttp pages).
- The redirect policy function refuses a foreign-host hop and allows a same-host hop (pure
  unit tests), and the declared client is constructed with the confined host.
