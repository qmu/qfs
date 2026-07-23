---
created_at: 2026-07-23T09:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Added
depends_on:
mission: a-request-resolves-to-a-principal-the-query-path-can-read
---

# serve the /sys read facet and resolve a session cookie to a principal

## Overview

Unblocks mission acceptance item 8. The in-container live round (ticket
20260719101204) proved the anonymous case but could not prove the session-carrying case,
because two pieces are un-shipped on this branch (verified 2026-07-23):

1. `crates/qfs/src/serve.rs` (via `serve_builtins::register_builtins`) registers the
   credential-free built-in read sources (`/status`, …) into the serve `ReadRegistry`, but
   **not** the `/sys` read driver — so an endpoint `AS /sys/whoami` is refused at registration
   (`UnroutedPath`) and `GET /whoami` 404s over the serve face.
2. `crates/http/src/handler.rs::resolve_request_principal` is a hardcoded stub:
   `fn resolve_request_principal(_req: &HttpRequest) -> RequestContext { RequestContext::anonymous() }`
   — it ignores the request and never reads the `qfs_session` cookie, so every request is
   anonymous even when it carries a valid session.

## Scope

1. **Register the `/sys` read facet on the serve face.** Add the `/sys` read driver
   (`SysReadDriver`, the one that already answers `/sys/whoami` in one-shot `qfs run`) to the
   serve-side `ReadRegistry` alongside the other built-ins, so `/sys/whoami` resolves over HTTP
   exactly as it does in one-shot execution. Keep it credential-free and always-available.
2. **Resolve the session cookie to a principal.** Implement `resolve_request_principal` to read
   the `qfs_session` cookie from the request, look the session up in the session store, and
   return a `RequestContext` carrying the resolved `UserId` when the session is valid; fall back
   to `RequestContext::anonymous()` when the cookie is absent, malformed, or unknown. This is the
   consumption side of the OAuth mint face that already issues the cookie.

## Policies

- Fail closed: an absent/invalid/expired session resolves to **anonymous**, never to a guessed or
  partial principal. "Not signed in" stays a first-class, correct answer.
- No new grammar and no change to the one-shot path: `/sys/whoami` already works in `qfs run`;
  this only makes the same facet reachable over the serve face and makes the handler read the
  cookie the mint face already sets.
- Credential-free: `/sys` carries no secrets; registering it must not require any connected
  account.

## Quality Gate

- A hermetic serve-boot test: with the `/sys` facet registered, an endpoint `AS /sys/whoami`
  registers without `UnroutedPath`, and `GET /whoami` returns `signed_in`/`user` (200, not 404).
- A hermetic handler test: a request carrying a valid `qfs_session` cookie resolves to the
  matching `UserId` (`signed_in=true`, `user=<id>`); a request with no cookie, or a
  malformed/unknown one, resolves anonymous (`signed_in=false`, `user=null`).
- `cargo test --workspace`, clippy `-D warnings`, `cargo fmt --all --check`, `gen-docs --check`
  all pass.
- After this lands, the live-round ticket (20260719101204) re-runs in the container and proves
  the session-carrying case end to end — that is where item 8 is finally ticked.
