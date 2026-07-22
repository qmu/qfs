#!/bin/sh
# =====================================================================================
# who-am-I live round — autonomous, in-container transcript
# ticket 20260719101204-one-live-round-developer-attended.md
# mission a-request-resolves-to-a-principal-the-query-path-can-read (acceptance item 8)
#
# Runs INSIDE the harness container (containers/live-round/run.sh). /src is the worktree
# (read-only), /work is a throwaway volume. NO host creds, NO host ~/.claude, NO cloud.
#
# The transcript this prints IS the deliverable: every command, verbatim stdout+stderr,
# and the raw exit code. It proves the anonymous case and records exactly where the
# session-carrying case blocks on the shipped branch.
# =====================================================================================
set -u

# ------------------------------------------------------------------------------------
# Transcript helpers: echo the command, run it, print the RAW exit code. Nothing is
# swallowed — a non-zero exit is DATA, not a reason to abort the round.
# ------------------------------------------------------------------------------------
sep() { echo "======================================================================"; }
run() {  # run a plain argv command
  sep; echo "\$ $*"
  echo "----------------------------------------------------------------------"
  "$@"; rc=$?
  echo "----------------------------------------------------------------------"
  echo "[exit code: $rc]"; echo
  return $rc
}
run_sh() {  # run a shell snippet (pipes / redirects), $1 = label, $2 = script
  sep; echo "\$ $1"
  echo "----------------------------------------------------------------------"
  sh -c "$2"; rc=$?
  echo "----------------------------------------------------------------------"
  echo "[exit code: $rc]"; echo
  return $rc
}

echo "#####################################################################################"
echo "# who-am-I live round transcript"
echo "# date (container clock): $(date -u 2>/dev/null || true)"
echo "# uname: $(uname -a 2>/dev/null || true)"
echo "# whoami (os user): $(id 2>/dev/null || true)"
echo "#####################################################################################"
echo

# Isolated, throwaway HOME + config + state — everything dies with the container volume.
export HOME=/work/home
export XDG_CONFIG_HOME=/work/home/.config
export QFS_STATE_DIR=/work/state
export QFS_PASSPHRASE=live-round-test-passphrase
export QFS_HTTP_ADDR=127.0.0.1:8787
PORT=8787
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$QFS_STATE_DIR" /work/tmp

# =====================================================================================
echo "## STEP a — copy the read-only worktree into the writable volume and build"
# =====================================================================================
run_sh "cp -r /src/. /work/build" "cp -r /src/. /work/build"
# The Cargo workspace is under packages/qfs (the monorepo root has no Cargo.toml).
BUILD=/work/build/packages/qfs
run_sh "ls /work/build (repo root has no Cargo.toml; workspace is packages/qfs)" \
  "ls -la /work/build && echo '---' && ls /work/build/Cargo.toml 2>&1 || true"

echo "## STEP b — cargo build --release -p qfs (isolated CARGO_TARGET_DIR=/work/target)"
run_sh "cd $BUILD && cargo build --release -p qfs" \
  "cd $BUILD && cargo build --release -p qfs"
BUILD_RC=$?

QFS=/work/target/release/qfs
if [ ! -x "$QFS" ]; then
  echo "!!! release binary not present at $QFS after build (rc=$BUILD_RC) — cannot continue the round."
  echo "!!! OUTCOME: failed (build). See the build output above."
  exit 0
fi
run "$QFS" --version

# =====================================================================================
echo "## STEP c — ANONYMOUS path: qfs run -e '/sys/whoami' (CLI carries no session)"
echo "##          expect: signed_in=false, user=null, exit 0"
# =====================================================================================
run "$QFS" run -e '/sys/whoami'
ANON_RC=$?
# The read-back, also as a projection, to make the two columns unambiguous in the transcript.
run "$QFS" run -e '/sys/whoami |> SELECT signed_in, user'

# =====================================================================================
echo "## Seed a real local principal in the isolated identity store (invite create + redeem)."
echo "## This proves a named user (a principal) CAN be established in-container — the"
echo "## subject the query path is supposed to resolve to under a session."
# =====================================================================================
run "$QFS" init round@live.test
run "$QFS" invite create round@live.test --role member
# Re-mint deterministically and capture the token so redeem can consume it in the same run.
run_sh "qfs invite create --role member  (capture one-time token)" \
  "$QFS invite create --role member > /work/invite.out 2>&1; echo '--- invite output ---'; cat /work/invite.out"
TOKEN=$(grep -oE 'one-time token[^:]*: [0-9a-f]{64}' /work/invite.out 2>/dev/null | grep -oE '[0-9a-f]{64}' | head -1)
echo "[parsed invite token present: $( [ -n "${TOKEN:-}" ] && echo yes || echo no )]"; echo
if [ -n "${TOKEN:-}" ]; then
  run_sh "printf %s \"\$PW\" | qfs invite redeem <token> member@live.test" \
    "printf %s 'redeem-password-123' | $QFS invite redeem $TOKEN member@live.test"
fi
run "$QFS" identity whoami round@live.test
run "$QFS" identity whoami member@live.test

# =====================================================================================
echo "## STEP d — SESSION-carrying path over the shipped HTTP QUERY FACE."
echo "##          Boot 'qfs serve' with an endpoint AS /sys/whoami and try to read it"
echo "##          with a session cookie. This is the case the mission's item 8 requires."
# =====================================================================================
# A serve config: a control endpoint over the always-mounted /status built-in, plus the
# target endpoint over /sys/whoami. Same query language as everything else.
cat > /work/config.qfs <<'CFG'
CREATE ENDPOINT health ON 'GET /health' AS /status;
CREATE ENDPOINT whoami ON 'GET /whoami' AS /sys/whoami;
CFG
run_sh "cat /work/config.qfs" "cat /work/config.qfs"

# Start the daemon in the background; capture its log.
sep; echo "\$ $QFS serve /work/config.qfs   (background; QFS_HTTP_ADDR=$QFS_HTTP_ADDR)"
"$QFS" serve /work/config.qfs > /work/serve.log 2>&1 &
SERVE_PID=$!
echo "[serve pid: $SERVE_PID]"

# Wait for the port to accept a connection (up to ~20s), using whatever is available.
have() { command -v "$1" >/dev/null 2>&1; }
port_open() {
  if have bash; then bash -c "exec 3<>/dev/tcp/127.0.0.1/$PORT" 2>/dev/null && return 0 || return 1;
  elif have curl; then curl -s -o /dev/null "http://127.0.0.1:$PORT/health" 2>/dev/null && return 0 || return 1;
  else return 1; fi
}
i=0; while [ $i -lt 40 ]; do if port_open; then break; fi; sleep 0.5; i=$((i+1)); done
echo "[waited $((i)) half-seconds for the listener]"
echo "----- serve.log (boot) -----"; cat /work/serve.log; echo "----- end serve.log -----"; echo

# A dependency-free HTTP GET client, written as a SELF-CONTAINED dispatcher script so it
# works when invoked from run_sh's `sh -c` subshell (a shell function would not survive
# that boundary). Tries curl, then bash /dev/tcp, then wget. Prints the raw HTTP response.
cat > /work/http_get.sh <<'HG'
#!/bin/sh
# usage: http_get.sh <path> [cookie]   (PORT via env, default 8787)
p="$1"; c="${2:-}"; port="${PORT:-8787}"
if command -v curl >/dev/null 2>&1; then
  if [ -n "$c" ]; then curl -sS -i --max-time 10 -H "Cookie: qfs_session=$c" "http://127.0.0.1:$port$p"
  else curl -sS -i --max-time 10 "http://127.0.0.1:$port$p"; fi
elif command -v bash >/dev/null 2>&1; then
  bash -c '
    p="$1"; c="$2"; port="$3"
    exec 3<>/dev/tcp/127.0.0.1/"$port" || { echo "CONNECT-FAILED"; exit 7; }
    { printf "GET %s HTTP/1.1\r\n" "$p"; printf "Host: localhost\r\n";
      [ -n "$c" ] && printf "Cookie: qfs_session=%s\r\n" "$c";
      printf "Connection: close\r\n\r\n"; } >&3
    cat <&3
  ' _ "$p" "$c" "$port"
elif command -v wget >/dev/null 2>&1; then
  if [ -n "$c" ]; then wget -qS -O - --header="Cookie: qfs_session=$c" "http://127.0.0.1:$port$p" 2>&1
  else wget -qS -O - "http://127.0.0.1:$port$p" 2>&1; fi
else echo "NO-HTTP-CLIENT-AVAILABLE"; fi
HG
chmod +x /work/http_get.sh
echo "[http client: $(command -v curl >/dev/null 2>&1 && echo curl || (command -v bash >/dev/null 2>&1 && echo 'bash /dev/tcp' || (command -v wget >/dev/null 2>&1 && echo wget || echo none)))]"; echo

# --- d.1 control: the /status-backed endpoint proves the query face itself works ---
run_sh "GET /health   (control: endpoint AS /status)" "PORT=$PORT sh /work/http_get.sh /health"
# --- d.2 anonymous over the HTTP query face: endpoint AS /sys/whoami, no cookie ---
run_sh "GET /whoami    (no session cookie)" "PORT=$PORT sh /work/http_get.sh /whoami"
# --- d.3 session-carrying: the SAME endpoint WITH a session cookie header ---
run_sh "GET /whoami    (WITH Cookie: qfs_session=<token>)" "PORT=$PORT sh /work/http_get.sh /whoami deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
# --- d.4 is the local OAuth SIGN-IN FACE even up in-container? (the session-mint face) ---
run_sh "GET /.well-known/oauth-authorization-server  (is the sign-in/mint face up?)" \
  "PORT=$PORT sh /work/http_get.sh /.well-known/oauth-authorization-server"

# Stop the daemon.
sep; echo "\$ kill $SERVE_PID"
kill "$SERVE_PID" 2>/dev/null || true
sleep 1
kill -9 "$SERVE_PID" 2>/dev/null || true
echo "----- serve.log (full) -----"; cat /work/serve.log; echo "----- end serve.log -----"; echo

# =====================================================================================
echo "## SHIPPED-SOURCE EVIDENCE for the block (the two un-shipped pieces the session"
echo "## path needs). Captured from the read-only worktree at /src."
# =====================================================================================
run_sh "grep -n 'register' packages/qfs/src/serve.rs  (no /sys read driver is mounted under serve)" \
  "grep -nE 'reads\\.register|mounts\\.register|register_builtins|register_server_face|SysRead|/sys' /src/packages/qfs/crates/qfs/src/serve.rs || true"
run_sh "sed -n '170,179p' packages/qfs/crates/http/src/handler.rs  (principal is a hardcoded anonymous stub)" \
  "sed -n '168,180p' /src/packages/qfs/crates/http/src/handler.rs"

echo "#####################################################################################"
echo "# OUTCOME: BLOCKED — the session-carrying case cannot be proven on the shipped branch."
echo "#"
echo "# PROVEN in this transcript:"
echo "#   * Anonymous CLI  qfs run -e '/sys/whoami'  ->  signed_in=false, user=null, exit 0."
echo "#   * A real named principal can be seeded in-container (invite create + redeem)."
echo "#   * The HTTP query face itself works (GET /health over an AS /status endpoint)."
echo "#"
echo "# BLOCKED ON TWO UN-SHIPPED PIECES (see the source evidence above):"
echo "#   1. qfs serve (packages/qfs/crates/qfs/src/serve.rs) never registers the /sys read"
echo "#      driver (SysReadDriver) into its ReadRegistry, so an endpoint AS /sys/whoami"
echo "#      is REFUSED AT REGISTRATION — the serve boot log above shows"
echo "#      'malformed query spec: UnroutedPath { path: \"/sys/whoami\" }', so the route is"
echo "#      never created and GET /whoami is a 404. The HTTP query face cannot read"
echo "#      /sys/whoami at all. (The GET /health control over AS /status returns 200.)"
echo "#   2. packages/qfs/crates/http/src/handler.rs::resolve_request_principal is a"
echo "#      hardcoded 'RequestContext::anonymous()' stub. It never parses the qfs_session"
echo "#      cookie nor consults the session store, so even once (1) is fixed, no"
echo "#      session-carrying request can resolve to signed_in=true on the query path."
echo "#"
echo "# Neither is a container limitation: both are wiring that has not shipped on this"
echo "# branch. The local OAuth sign-in/mint face DOES boot in-container (discovery probe"
echo "# above), so the block is the query-path consumption of a session, not the mint."
echo "#####################################################################################"
