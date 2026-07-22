---
created_at: 2026-07-19T10:12:04+09:00
author: a@qmu.jp
type: housekeeping
layer: [Infrastructure]
effort: 1h
commit_hash:
category: Changed
depends_on:
mission: a-request-resolves-to-a-principal-the-query-path-can-read
---

# One live round — autonomous, in an isolated container, transcript recorded

Satisfies mission acceptance item 8. **Re-ruled by the owner, 2026-07-22** (overnight-run
directive: nothing in the night's queue waits for the developer): the round runs
**autonomously inside a container** (`docker` on this host is podman 5.8.4), and the
developer reviews the **recorded transcript** in the morning instead of attending the run.
The original developer-attended framing is superseded by this ruling; the evidence bar is
unchanged — raw output and raw exit codes, pasted.

## Steps (autonomous, in-container)

Run every step inside a fresh container (no host `~/.claude`, no host sockets; the repo
mounted or copied in is fine — this round touches qfs only, not Claude Code):

1. Build the release binary from this branch.
2. Anonymous path: `qfs run -e '/sys/whoami'` (or the HTTP query face with no session
   cookie) — expect the explicit not-signed-in answer (`signed_in=false, user=null`-shaped),
   exit 0. Record output + exit code verbatim.
3. Session path: seed the identity store (invite redeem or the test seam the branch
   provides), mint a session through the OAuth sign-in face (`qfs/src/oauth.rs`), then issue
   a query request carrying that session cookie — expect the named principal
   (`signed_in=true, user=<id>`). Record output + exit code verbatim.
4. Policy both-directions (recommended): with a `FOR <user>` narrowed rule bound, show it
   bites under the session and contributes nothing anonymous. Record both transcripts.
5. Paste the full transcript (commands, outputs, exit codes) into this ticket's Final Report
   and the PR story — that transcript is the deliverable the developer reviews.

If a step cannot be completed in-container, record it `blocked` with the exact missing piece
— do not run it on the shared host, and do not wait for the developer mid-run.

## Policies

**運用 / `workaholic:operation`**
- `ci-cd` / ship-on-real-response — "Ground the decision to ship not in the fact that the
  process turned green but in the fact that production actually responds as expected." The
  recorded live round is that ground for this mission; the developer's looking-through
  relocates to the morning transcript review (owner re-ruling, 2026-07-22).

**設計 / `workaholic:design`**
- `access-control` — the round proves resolution happens under the caller's principal on the
  real path, not only in tests.

**安全**
- Isolated container only; never mount the host `~/.claude` or its sockets. This round needs
  neither — it exercises qfs alone.

## Quality Gate

**Acceptance criteria.** Two real requests (session + no session) resolve to their
respective answers through the shipped path; the raw transcript (output + exit codes) is
pasted into the ticket Final Report and PR.

**Verification method.** The autonomous leaf runs the commands in the container and records
the transcript verbatim; the developer reviews it in the morning.

**Gate that must pass.** The transcript shows the correct answers and exit codes; the branch
gates (build/test/clippy/fmt/xtask) green.

## Final Report — 2026-07-22 (autonomous, in-container)

**Outcome: `blocked`.** The anonymous case is proven; the session-carrying case cannot be
proven on the shipped branch and is blocked on two un-shipped pieces (below). Per this
ticket's own instruction ("If a step cannot be completed in-container, record it `blocked`
with the exact missing piece — do not run it on the shared host, and do not wait for the
developer mid-run"), the round did not attempt a host run and did not escalate. Mission
acceptance item 8 is **not** ticked and the ticket is **not** archived.

### How it was run

Inside the prepared harness (`containers/live-round/run.sh` -> `containers/live-round/round.sh`),
podman 5.8.4, `rust:1-slim-bookworm`, worktree bind-mounted read-only at `/src`, a throwaway
`/work` volume for the build + an isolated HOME/System-DB/state dir. No host `~/.claude`, no
host sockets, no cloud credentials (the round needs none). The release binary was built from
this branch (`cargo build --release -p qfs`, finished in 3m36s). `qfs 0.0.81`.

One harness fix was required: `run.sh`'s round-script branch execed `"$IMAGE" sh "/round/$ROUND"`,
but the Containerfile `ENTRYPOINT` is `["/bin/sh"]`, so the container ran `/bin/sh sh /round/round.sh`
-> `cannot open sh`. Corrected to `"$IMAGE" "/round/$ROUND"` (image, mounts, volume, and every
safety invariant unchanged).

### What is proven

- **Anonymous, CLI** - `qfs run -e '/sys/whoami'` -> `{"signed_in":false,"user":null}`, exit 0.
- A real named principal **can** be seeded in-container (`qfs init`, `qfs invite create` +
  `qfs invite redeem` -> users 1 and 2 with a usable password) - the subject the query path is
  meant to resolve to.
- The HTTP query face itself works: `GET /health` over an `AS /status` endpoint -> `200 OK`.
- The local OAuth sign-in / session-mint face **is up** in-container:
  `GET /.well-known/oauth-authorization-server` -> `200 OK`. So the block is NOT an inability
  to stand up the mint; it is the query path's inability to *consume* a session.

### Where it blocks (the exact missing pieces)

1. **`qfs serve` does not mount `/sys`.** `packages/qfs/crates/qfs/src/serve.rs` registers only
   the `/status` built-in, the `/claude` facade, and the `/server` self-config face - never the
   `/sys` read driver (`SysReadDriver`). So `CREATE ENDPOINT whoami ... AS /sys/whoami` is
   **refused at registration** - the serve boot log shows
   `endpoint 'whoami' has a malformed query spec: UnroutedPath { path: "/sys/whoami" }` - the
   route is never created and `GET /whoami` returns `404`. The HTTP query face cannot read
   `/sys/whoami` at all.
2. **The HTTP handler never reads the session cookie.**
   `packages/qfs/crates/http/src/handler.rs::resolve_request_principal` is a hardcoded
   `RequestContext::anonymous()` stub (its own doc-comment names this the drop-in deferred to
   "mission item 8"). It never parses the `qfs_session` cookie nor consults a session store, so
   even once (1) is fixed, no session-carrying request can resolve to `signed_in=true` on the
   query path.

Neither is a container limitation - both are wiring that has not shipped on this branch.
Implementing them is out of scope for this transcript/live-round ticket and needs its own
implementation ticket. Verbatim runtime transcript (commands, outputs, exit codes) follows; the
complete raw file including the 3m36s build log is committed at
`containers/live-round/transcript.txt`.

```text
======================================================================
$ /work/target/release/qfs --version
----------------------------------------------------------------------
qfs 0.0.81
commit:  unknown
target:  aarch64-unknown-linux-gnu
wasm32:  false
----------------------------------------------------------------------
[exit code: 0]

## STEP c — ANONYMOUS path: qfs run -e '/sys/whoami' (CLI carries no session)
##          expect: signed_in=false, user=null, exit 0
======================================================================
$ /work/target/release/qfs run -e /sys/whoami
----------------------------------------------------------------------
{"schema":[{"name":"signed_in","type":"bool"},{"name":"user","type":"text"}],"rows":[{"signed_in":false,"user":null}],"meta":{"row_count":1,"truncated":false,"limit":null,"offset":null,"affected":null}}
----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ /work/target/release/qfs run -e /sys/whoami |> SELECT signed_in, user
----------------------------------------------------------------------
{"schema":[{"name":"signed_in","type":"bool"},{"name":"user","type":"text"}],"rows":[{"signed_in":false,"user":null}],"meta":{"row_count":1,"truncated":false,"limit":null,"offset":null,"affected":null}}
----------------------------------------------------------------------
[exit code: 0]

## Seed a real local principal in the isolated identity store (invite create + redeem).
## This proves a named user (a principal) CAN be established in-container — the
## subject the query path is supposed to resolve to under a session.
======================================================================
$ /work/target/release/qfs init round@live.test
----------------------------------------------------------------------
qfs is ready: operator round@live.test; vault unlocked (1 key slot(s): passphrase)
----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ /work/target/release/qfs invite create round@live.test --role member
----------------------------------------------------------------------
error: unexpected argument 'round@live.test' found

Usage: qfs invite create [OPTIONS]

For more information, try '--help'.
----------------------------------------------------------------------
[exit code: 2]

======================================================================
$ qfs invite create --role member  (capture one-time token)
----------------------------------------------------------------------
--- invite output ---
invite 1 created (expires 2026-07-29T12:05:35Z, scope host, role member).
one-time token (store it now — it is shown only once): 910109a90f6de751ab6dee78d4886cb5cb41e50399ba45839baabd2409c72344
redeem with: printf %s "$PASSWORD" | qfs invite redeem 910109a90f6de751ab6dee78d4886cb5cb41e50399ba45839baabd2409c72344 <email>
(email delivery is a seam; hand out this URL/token out of band until mail is wired)
----------------------------------------------------------------------
[exit code: 0]

[parsed invite token present: yes]

======================================================================
$ printf %s "$PW" | qfs invite redeem <token> member@live.test
----------------------------------------------------------------------
redeemed: member@live.test is now user 2 and a member member (membership 1)
----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ /work/target/release/qfs identity whoami round@live.test
----------------------------------------------------------------------
round@live.test (user 1)
----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ /work/target/release/qfs identity whoami member@live.test
----------------------------------------------------------------------
member@live.test (user 2)
----------------------------------------------------------------------
[exit code: 0]

## STEP d — SESSION-carrying path over the shipped HTTP QUERY FACE.
##          Boot 'qfs serve' with an endpoint AS /sys/whoami and try to read it
##          with a session cookie. This is the case the mission's item 8 requires.
======================================================================
$ cat /work/config.qfs
----------------------------------------------------------------------
CREATE ENDPOINT health ON 'GET /health' AS /status;
CREATE ENDPOINT whoami ON 'GET /whoami' AS /sys/whoami;
----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ /work/target/release/qfs serve /work/config.qfs   (background; QFS_HTTP_ADDR=127.0.0.1:8787)
[serve pid: 4746]
[waited 1 half-seconds for the listener]
----- serve.log (boot) -----
[2m2026-07-22T12:05:35.959261Z[0m [33m WARN[0m [2mqfs::http[0m[2m:[0m endpoint not registered (policy/compile refusal) [3mendpoint[0m[2m=[0mwhoami [3mreason[0m[2m=[0mendpoint `whoami` has a malformed query spec: UnroutedPath { path: "/sys/whoami" }
[2m2026-07-22T12:05:35.959321Z[0m [33m WARN[0m [2mqfs::http[0m[2m:[0m endpoint not registered (policy/compile refusal) [3mendpoint[0m[2m=[0mwhoami [3mreason[0m[2m=[0mendpoint `whoami` has a malformed query spec: UnroutedPath { path: "/sys/whoami" }
----- end serve.log -----

[http client: bash /dev/tcp]

======================================================================
$ GET /health   (control: endpoint AS /status)
----------------------------------------------------------------------
HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: 190
Connection: close

{"schema":[{"name":"ok","type":"int"},{"name":"service","type":"text"}],"rows":[{"ok":1,"service":"qfs"}],"meta":{"row_count":1,"truncated":false,"limit":null,"offset":null,"affected":null}}----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ GET /whoami    (no session cookie)
----------------------------------------------------------------------
HTTP/1.1 404 Not Found
Content-Type: application/json
Content-Length: 73
Connection: close

{"error":"not_found","detail":"no endpoint matches this method and path"}----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ GET /whoami    (WITH Cookie: qfs_session=<token>)
----------------------------------------------------------------------
HTTP/1.1 404 Not Found
Content-Type: application/json
Content-Length: 73
Connection: close

{"error":"not_found","detail":"no endpoint matches this method and path"}----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ GET /.well-known/oauth-authorization-server  (is the sign-in/mint face up?)
----------------------------------------------------------------------
HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: 385
Connection: close

{"issuer":"http://localhost:8787","jwks_uri":"http://localhost:8787/jwks.json","response_types_supported":["code"],"code_challenge_methods_supported":["S256"],"grant_types_supported":["authorization_code","refresh_token"],"authorization_endpoint":"http://localhost:8787/authorize","token_endpoint":"http://localhost:8787/token","registration_endpoint":"http://localhost:8787/register"}----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ kill 4746
----- serve.log (full) -----
[2m2026-07-22T12:05:35.959261Z[0m [33m WARN[0m [2mqfs::http[0m[2m:[0m endpoint not registered (policy/compile refusal) [3mendpoint[0m[2m=[0mwhoami [3mreason[0m[2m=[0mendpoint `whoami` has a malformed query spec: UnroutedPath { path: "/sys/whoami" }
[2m2026-07-22T12:05:35.959321Z[0m [33m WARN[0m [2mqfs::http[0m[2m:[0m endpoint not registered (policy/compile refusal) [3mendpoint[0m[2m=[0mwhoami [3mreason[0m[2m=[0mendpoint `whoami` has a malformed query spec: UnroutedPath { path: "/sys/whoami" }
----- end serve.log -----

## SHIPPED-SOURCE EVIDENCE for the block (the two un-shipped pieces the session
## path needs). Captured from the read-only worktree at /src.
======================================================================
$ grep -n 'register' packages/qfs/src/serve.rs  (no /sys read driver is mounted under serve)
----------------------------------------------------------------------
33:    crate::serve_builtins::register_builtins(&mut engine, &mut reads);
50:        reads.register(
63:    crate::server_face::register_server_face(&mut engine, &mut reads, &server_state);
----------------------------------------------------------------------
[exit code: 0]

======================================================================
$ sed -n '170,179p' packages/qfs/crates/http/src/handler.rs  (principal is a hardcoded anonymous stub)
----------------------------------------------------------------------
}

/// Resolve the request's [`RequestContext`] — the M2 "who am I" seam threaded to the gate and the
/// read executor. The session cookie → `UserId` resolution needs an injected session store (built
/// at serve boot); that end-to-end binding lands with the developer-attended live round (mission
/// item 8). Until then a request carries no principal the handler can VERIFY, so it resolves to the
/// anonymous (not-signed-in) actor — the fail-closed default. A cookie that cannot be verified
/// grants nothing; wiring the seam through the handler is what makes item 8 a drop-in, not a
/// re-plumb.
fn resolve_request_principal(_req: &HttpRequest) -> RequestContext {
    RequestContext::anonymous()
}

----------------------------------------------------------------------
[exit code: 0]

#####################################################################################
# OUTCOME: BLOCKED — the session-carrying case cannot be proven on the shipped branch.
#
# PROVEN in this transcript:
#   * Anonymous CLI  qfs run -e '/sys/whoami'  ->  signed_in=false, user=null, exit 0.
#   * A real named principal can be seeded in-container (invite create + redeem).
#   * The HTTP query face itself works (GET /health over an AS /status endpoint).
#
# BLOCKED ON TWO UN-SHIPPED PIECES (see the source evidence above):
#   1. qfs serve (packages/qfs/crates/qfs/src/serve.rs) never registers the /sys read
#      driver (SysReadDriver) into its ReadRegistry, so an endpoint AS /sys/whoami
#      is REFUSED AT REGISTRATION — the serve boot log above shows
#      'malformed query spec: UnroutedPath { path: "/sys/whoami" }', so the route is
#      never created and GET /whoami is a 404. The HTTP query face cannot read
#      /sys/whoami at all. (The GET /health control over AS /status returns 200.)
#   2. packages/qfs/crates/http/src/handler.rs::resolve_request_principal is a
#      hardcoded 'RequestContext::anonymous()' stub. It never parses the qfs_session
#      cookie nor consults the session store, so even once (1) is fixed, no
#      session-carrying request can resolve to signed_in=true on the query path.
#
# Neither is a container limitation: both are wiring that has not shipped on this
# branch. The local OAuth sign-in/mint face DOES boot in-container (discovery probe
# above), so the block is the query-path consumption of a session, not the mint.
#####################################################################################
RUN EXIT: 0
```
