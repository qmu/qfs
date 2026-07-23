#!/bin/sh
# =====================================================================================
# who-am-I live round — autonomous, in-container transcript
# ticket 20260719101204-one-live-round-developer-attended.md
# mission a-request-resolves-to-a-principal-the-query-path-can-read (acceptance item 8)
#
# Runs INSIDE the harness container (containers/live-round/run.sh). /src is the worktree
# (read-only), /work is a throwaway RAM tmpfs. NO host creds, NO host ~/.claude, NO cloud.
#
# The transcript this prints IS the deliverable: every command, verbatim stdout+stderr,
# and the raw exit code. It proves BOTH the anonymous case (signed_in=false) and the
# session-carrying case (signed_in=true) end to end over the shipped HTTP query face,
# using a REAL qfs_session cookie minted through the local OAuth sign-in POST.
#
# The two pieces the earlier round blocked on are now shipped on this branch (b4f1997):
#   1. qfs serve registers the credential-free /sys read facet, so AS /sys/whoami resolves
#      over HTTP (no more UnroutedPath; GET /whoami is 200, not 404).
#   2. resolve_request_principal reads the qfs_session cookie via an injected resolver over
#      the System-DB session store, mapping a live session to its UserId (else anonymous).
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

# Isolated, throwaway HOME + config + state — everything dies with the container tmpfs.
export HOME=/work/home
export XDG_CONFIG_HOME=/work/home/.config
export QFS_STATE_DIR=/work/state
export QFS_PASSPHRASE=live-round-test-passphrase
export QFS_HTTP_ADDR=127.0.0.1:8787
PORT=8787
BASE="http://127.0.0.1:8787"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$QFS_STATE_DIR" /work/tmp

# =====================================================================================
echo "## STEP a — copy the read-only worktree into the writable tmpfs and build"
# =====================================================================================
run_sh "cp -r /src/. /work/build" "cp -r /src/. /work/build"
# The Cargo workspace is under packages/qfs (the monorepo root has no Cargo.toml).
BUILD=/work/build/packages/qfs
run_sh "ls /work/build (repo root has no Cargo.toml; workspace is packages/qfs)" \
  "ls -la /work/build && echo '---' && ls /work/build/packages/qfs/Cargo.toml 2>&1 || true"

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
run "$QFS" run -e '/sys/whoami |> SELECT signed_in, user'

# =====================================================================================
echo "## Seed a real local principal in the isolated identity store (init + invite redeem)."
echo "## member@live.test becomes user 2 with a usable password — the subject the query"
echo "## path must resolve to under a session cookie."
# =====================================================================================
run "$QFS" init round@live.test
PW='redeem-password-123'
run_sh "qfs invite create --role member  (capture one-time token)" \
  "$QFS invite create --role member > /work/invite.out 2>&1; echo '--- invite output ---'; cat /work/invite.out"
TOKEN=$(grep -oE '[0-9a-f]{64}' /work/invite.out 2>/dev/null | head -1)
echo "[parsed invite token present: $( [ -n "${TOKEN:-}" ] && echo yes || echo no )]"; echo
if [ -n "${TOKEN:-}" ]; then
  run_sh "printf %s \"\$PW\" | qfs invite redeem <token> member@live.test" \
    "printf %s '$PW' | $QFS invite redeem $TOKEN member@live.test"
fi
run "$QFS" identity whoami round@live.test
run "$QFS" identity whoami member@live.test

# =====================================================================================
echo "## STEP d — SESSION-carrying path over the shipped HTTP QUERY FACE."
echo "##          Boot 'qfs serve' with an endpoint AS /sys/whoami, mint a REAL session"
echo "##          cookie through the local OAuth sign-in POST, and read /whoami with it."
# =====================================================================================
cat > /work/config.qfs <<'CFG'
CREATE ENDPOINT health ON 'GET /health' AS /status;
CREATE ENDPOINT whoami ON 'GET /whoami' AS /sys/whoami;
CFG
run_sh "cat /work/config.qfs" "cat /work/config.qfs"

# Start the daemon in the background; capture its log. QFS_PASSPHRASE is exported so the
# OAuth AS (the session-mint face) boots over the same System DB the resolver reads.
sep; echo "\$ $QFS serve /work/config.qfs   (background; QFS_HTTP_ADDR=$QFS_HTTP_ADDR)"
"$QFS" serve /work/config.qfs > /work/serve.log 2>&1 &
SERVE_PID=$!
echo "[serve pid: $SERVE_PID]"

# Wait for the listener to accept a connection (up to ~30s).
i=0; while [ $i -lt 60 ]; do
  if curl -sS -o /dev/null "$BASE/health" 2>/dev/null; then break; fi
  sleep 0.5; i=$((i+1))
done
echo "[waited $i half-seconds for the listener]"
echo "----- serve.log (boot) -----"; cat /work/serve.log; echo "----- end serve.log -----"; echo
echo "[registration check: the fix means NO 'UnroutedPath' / 'malformed query spec' for whoami above]"; echo

# --- d.1 control: the /status-backed endpoint proves the query face itself works ---
run_sh "GET /health   (control: endpoint AS /status)" \
  "curl -sS -i --max-time 10 $BASE/health; echo"

# --- d.2 anonymous over the HTTP query face: endpoint AS /sys/whoami, NO cookie ---
run_sh "GET /whoami    (no session cookie -> expect signed_in=false, user=null)" \
  "curl -sS -i --max-time 10 $BASE/whoami | tee /work/anon.out; echo"

# --- d.3 register a public OAuth client (RFC 7591 dynamic registration) ---
run_sh "POST /register (dynamic client registration)" \
  "curl -sS -i --max-time 10 -X POST -H 'Content-Type: application/json' \
     --data '{\"redirect_uris\":[\"$BASE/callback\"],\"client_name\":\"live-round\"}' \
     $BASE/register | tee /work/register.out; echo"
CID=$(grep -oE '\"client_id\"[[:space:]]*:[[:space:]]*\"[^\"]+\"' /work/register.out 2>/dev/null \
      | sed -E 's/.*\"client_id\"[[:space:]]*:[[:space:]]*\"([^\"]+)\".*/\1/' | head -1)
echo "[parsed client_id present: $( [ -n "${CID:-}" ] && echo yes || echo no )]"; echo

# --- d.4 sign-in POST: authenticate member@live.test and MINT a qfs_session cookie ---
# The POST /authorize sign-in leg verifies the local password, mints a session, and returns
# it as Set-Cookie on the 302. code_challenge is any non-empty S256-declared value (we want
# only the minted cookie, not the token exchange). redirect_uri EXACT-matches registration.
FORM="response_type=code&client_id=${CID:-MISSING}&redirect_uri=http%3A%2F%2F127.0.0.1%3A8787%2Fcallback&scope=mcp%3Aread&state=live-round-state&code_challenge=liveround0challenge0placeholder0value00000000&code_challenge_method=S256&email=member%40live.test&password=redeem-password-123&decision=approve"
run_sh "POST /authorize (sign-in: verify password + mint session -> 302 Set-Cookie)" \
  "curl -sS -i --max-time 10 -X POST -H 'Content-Type: application/x-www-form-urlencoded' \
     --data '$FORM' $BASE/authorize | tee /work/authorize.out; echo"
SESSION=$(grep -i '^set-cookie:' /work/authorize.out 2>/dev/null \
          | sed -n 's/.*qfs_session=\([^;]*\).*/\1/p' | head -1)
echo "[minted qfs_session cookie present: $( [ -n "${SESSION:-}" ] && echo yes || echo no )]"; echo

# --- d.5 session-carrying: the SAME /whoami endpoint WITH the minted session cookie ---
run_sh "GET /whoami    (WITH Cookie: qfs_session=<minted> -> expect signed_in=true, user=2)" \
  "curl -sS -i --max-time 10 -H 'Cookie: qfs_session=${SESSION:-none}' $BASE/whoami | tee /work/session.out; echo"

# Stop the daemon.
sep; echo "\$ kill $SERVE_PID"
kill "$SERVE_PID" 2>/dev/null || true
sleep 1
kill -9 "$SERVE_PID" 2>/dev/null || true
echo "----- serve.log (full) -----"; cat /work/serve.log; echo "----- end serve.log -----"; echo

# =====================================================================================
# OUTCOME — computed from the two captured responses (not hardcoded).
# =====================================================================================
ANON_OK=no;    grep -q '"signed_in":false' /work/anon.out    2>/dev/null && grep -q '"user":null' /work/anon.out 2>/dev/null && ANON_OK=yes
SESSION_OK=no; grep -q '"signed_in":true'  /work/session.out 2>/dev/null && grep -q '"user":"2"'  /work/session.out 2>/dev/null && SESSION_OK=yes

echo "#####################################################################################"
if [ "$ANON_OK" = yes ] && [ "$SESSION_OK" = yes ]; then
  echo "# OUTCOME: PROVEN — both requests resolve to their principal on the shipped query path."
else
  echo "# OUTCOME: NOT fully proven (see below) — inspect the captured responses above."
fi
echo "#"
echo "# ANONYMOUS  GET /whoami (no cookie)        -> signed_in=false, user=null : $ANON_OK"
echo "# SESSION    GET /whoami (qfs_session cookie) -> signed_in=true,  user=2   : $SESSION_OK"
echo "#"
echo "# The session cookie was minted through the LOCAL OAuth sign-in POST (POST /authorize"
echo "# verifying member@live.test's password), the same System-DB session store the serve"
echo "# resolver reads. No credentials, no cloud, no host state — entirely in-container."
echo "#####################################################################################"
