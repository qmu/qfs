#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# The npx smoke check.
#
# The product's headline promise is `npx qfs-viewer` at a repository root.
# Nothing else in the gate exercises it: the unit suite runs TS source
# directly, so the bin, the `files` list, and the launcher could all be broken
# while every other check stayed green.
#
# This packs the package exactly as the registry would serve it, installs the
# tarball into a scratch tree (so the bin really does live under
# `node_modules`), and runs it. That is what makes it a real check: Node 24
# refuses to strip types from `.ts` under `node_modules`, so a launcher
# missing the relocate step fails HERE and nowhere else.
#
# WHAT THIS CHECK NO LONGER PROVES
# --------------------------------
# The install below sets `min-release-age=0`, so this smoke resolves releases
# the developer's `~/.npmrc` floor would hide. That is a deliberate trade,
# made 2026-07-15 (see docs/adr/0005), and it costs a real signal:
#
#   this smoke NO LONGER proves that a consumer who respects a
#   min-release-age floor can install the product.
#
# It still proves everything it was built for — that the packed `files` list,
# the `bin` entry, and the Node-24 launcher survive being installed under
# `node_modules` — which is the failure mode nothing else in the gate can see.
# Those two concerns were only ever conflated here by accident: a dependency
# being too young is a release-readiness fact, not a packaging fault, and
# leaving the gate red on it would have taught everyone to ignore a red gate.
#
# The cost is that a genuinely unresolvable dependency now passes this check.
# Guard it at release time instead: before publishing or shipping, run the
# install WITHOUT the override and confirm it resolves —
#
#   (cd "$(mktemp -d)" && npm init -y >/dev/null && npm install qfs-viewer)
#
# REMOVE THE OVERRIDE once the pinned plgg-md clears the floor: currently
# 0.0.3, so 2026-07-22 21:09 JST. That date has ALREADY MOVED once — it was
# 2026-07-16 against 0.0.2, and adopting the upstream YAML-subset fix (0.0.3)
# restarted the seven-day clock. Every fix we ask for and adopt pushes it out
# another week, which is precisely how a time-boxed workaround becomes
# permanent. docs/adr/0005 records when to stop extending rather than keep
# bumping this comment.
#
# It is time-boxed debt like `scripts/plgg-tool.sh` — with the difference,
# stated plainly, that the relocate bridge masks no failure while this one does.
#
# WHAT IT DOES PROVE, SINCE 2026-07-17
# ------------------------------------
# `--version` and `--help` were the whole of it, and that was never the
# promise: `npx qfs-viewer` means SERVE. A bin that printed its version and
# then failed to serve — a UI engine that does not resolve for a real
# consumer, a renderer that throws on first request — passed this check
# completely. The strip landed on the published `plggmatic` (PR #9) and
# nothing in the gate would have noticed the engine failing to resolve from
# under a registry install, which is the one place it can fail.
#
# So each runtime now also STARTS the packed bin and is asked, over HTTP,
# whether it serves the engine strip (scripts/smoke-serve-assert.mjs). The
# scratch tree's qfs is configured to a binary that cannot exist, which makes
# the second half of the acceptance checkable in the same run: with no qfs
# reachable the viewer must start anyway and say what is missing and how to
# get it (docs/adr/0009), rather than crash.
echo "=== Smoke: npx qfs-viewer (packed, installed, executed) ==="

PACKAGE_DIR="$REPO_ROOT/packages/qfs-viewer"
EXPECTED=$(node -p "require('$PACKAGE_DIR/package.json').version")

SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/qfs-viewer-smoke-XXXXXX")
# Clean up on every exit path, including a failed assertion — and kill a
# serve that is still running, or a failed assertion leaves a server holding
# a port for as long as the shell lives.
SERVE_PID=""

# Stop the serve and everything it spawned.
#
# `pkill -P` before `kill`, and it is not belt-and-braces: the launcher
# RELOCATES (bin/relocate.mjs — Node 24 refuses to strip types from `.ts`
# under node_modules) and re-execs the real server as a CHILD it waits on
# synchronously. Signal the launcher alone and the child survives, still
# holding the port — measured here, not feared, and left as a finding on
# `20260717153000-a-relocated-serve-outlives-its-launcher.md` rather than
# papered over: a viewer a process manager cannot stop is the product's
# problem, not this script's.
stop_serve() {
  if [ -z "$SERVE_PID" ]; then
    return 0
  fi
  pkill -TERM -P "$SERVE_PID" 2>/dev/null || true
  kill "$SERVE_PID" 2>/dev/null || true
  wait "$SERVE_PID" 2>/dev/null || true
  SERVE_PID=""
}

cleanup() {
  stop_serve
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

# 1. Pack as the registry would.
TARBALL=$(cd "$PACKAGE_DIR" && npm pack --silent --pack-destination "$SCRATCH")

# 2. Install the tarball into a scratch consumer, so the bin runs from under
#    node_modules — the condition the launcher must survive.
#
#    `min-release-age=0` is scoped to this one command: it is not exported, so
#    nothing else in the gate — and nothing in `~/.npmrc` — is touched. See the
#    header for what this costs and when to remove it.
#
#    stderr is NOT swallowed. It was, and that hid a real failure: when this
#    install died on ETARGET the gate exited 1 having printed nothing at all,
#    so the one line saying WHY was thrown away and the smoke looked like a
#    launcher fault. A check that cannot say why it failed is barely a check.
cd "$SCRATCH"
npm init -y >/dev/null 2>&1

# The scratch tree gets a corpus and a config, because `serve` is only
# testable against something to serve.
#
# ONE markdown document, so `/api/health` reporting `documentCount: 1` is a
# fact about THIS tree and not about whatever the runner's node_modules
# happened to contain (the scan prunes node_modules — see Scan.ts, where the
# same mistake is recorded costing a 1714-document corpus its meaning).
mkdir -p docs
cat > docs/smoke.md <<'MARKDOWN'
---
type: enhancement
layer: [Infrastructure]
---

# Smoke document

SMOKE-DOCUMENT-BODY
MARKDOWN

# The config points qfs at a binary that CANNOT exist. That is the acceptance
# clause "with no qfs reachable" made deterministic: stripping PATH would
# take the runtime with it, and trusting the runner's machine not to have a
# qfs would make this check pass or fail by accident. A named-but-absent
# binary walks the same code path — `execFileSync` ENOENT — as an empty PATH,
# and it walks it on every machine.
cat > qfs-viewer.config.json <<'JSON'
{
  "title": "smoke corpus",
  "qfs": {
    "form": "spawn",
    "bin": "/nonexistent/qfs-not-installed-here"
  }
}
JSON
if ! NPM_CONFIG_MIN_RELEASE_AGE=0 \
  npm install --no-audit --no-fund "$SCRATCH/$TARBALL" >/dev/null; then
  echo "  FAIL: the packed tarball could not be installed (see npm's error above)" >&2
  exit 1
fi

# 3. Run the installed bin under EVERY runtime available.
#
#    The mission requires node, bun AND deno. This checked node only, and so it
#    passed for a whole session while bun could not run the product at all --
#    the self-alias needs a resolver, and each runtime had a different one, so
#    "it resolves" was true once per runtime rather than once.
#
#    That is fixed at the root now: the self-alias is `#qfs-viewer/*` in
#    package.json's `imports`, which node, bun, deno and tsc all implement.
#    One mechanism, so this loop tests one thing three times instead of three
#    things once each.
#
#    A runtime that is not installed is SKIPPED OUT LOUD. A silent skip is how
#    this regresses unseen, which is the failure this whole step exists to
#    stop.
RAN=0
for RUNTIME in node bun deno; do
  if ! command -v "$RUNTIME" >/dev/null 2>&1; then
    echo "  SKIP: $RUNTIME is not installed — this check did NOT cover it"
    continue
  fi

  case "$RUNTIME" in
    deno) RUN="deno run -A" ;;
    *) RUN="$RUNTIME" ;;
  esac

  # Capture stderr and the STATUS rather than discarding both. Both halves of
  # that were load-bearing, and this line used to do neither:
  #
  #   - `2>/dev/null` threw away the one thing that says why a runtime cannot
  #     run the product.
  #   - a bare `ACTUAL=$(...)` under `sh -eu` is itself a checked command, so a
  #     non-zero runtime exited the whole script HERE — before the FAIL echo
  #     below could run. deno was broken for a session and this step printed
  #     nothing at all: not the error, not even its own failure line.
  #
  # So the check that exists to prove every runtime runs the bin was, on the
  # runtime that did not, silent. Per this file's own header: a check that
  # cannot say why it failed is barely a check.
  if ! ACTUAL=$($RUN ./node_modules/.bin/qfs-viewer --version 2>&1); then
    echo "  FAIL: $RUNTIME could not run the packed bin:" >&2
    echo "$ACTUAL" | sed 's/^/    /' >&2
    exit 1
  fi
  if [ "$ACTUAL" != "$EXPECTED" ]; then
    echo "  FAIL: $RUNTIME: --version printed \"$ACTUAL\", expected \"$EXPECTED\"" >&2
    exit 1
  fi

  # The help surface answers too (argv handling reached, not just the version
  # shortcut).
  if ! HELP_ERR=$($RUN ./node_modules/.bin/qfs-viewer --help 2>&1 >/dev/null); then
    echo "  FAIL: $RUNTIME: --version answered but --help did not:" >&2
    echo "$HELP_ERR" | sed 's/^/    /' >&2
    exit 1
  fi

  # THE SERVE PATH — the promise itself. Everything above proves the bin
  # answers; this proves it SERVES, which is what `npx qfs-viewer` means.
  #
  # An EPHEMERAL port, asked of the OS rather than picked: this box already
  # runs the development workload on 4100, and a hard-coded port makes the
  # gate fail for the developer whose viewer is up — the exact failure that
  # teaches people to ignore a red gate. The bind-and-release leaves a race
  # (the OS may hand the port to someone else before the server claims it)
  # that is a few milliseconds wide and, when it is lost, fails loudly here
  # rather than passing quietly.
  PORT=$(node -e 'const s=require("node:net").createServer();s.listen(0,"127.0.0.1",()=>{const p=s.address().port;s.close(()=>process.stdout.write(String(p)))})')
  SERVE_LOG="$SCRATCH/serve-$RUNTIME.log"

  # stdout AND stderr to the log, and the log is evidence: the assertions
  # read it for the boot's `qfs.unreachable` line, and print the whole thing
  # when the server never answers. A crash on boot is otherwise a timeout
  # with no cause attached.
  $RUN ./node_modules/.bin/qfs-viewer serve --port "$PORT" \
    > "$SERVE_LOG" 2>&1 &
  SERVE_PID=$!

  if ! node "$REPO_ROOT/scripts/smoke-serve-assert.mjs" \
    "$PORT" "$SERVE_LOG" "docs/smoke.md"; then
    echo "  FAIL: $RUNTIME packed the bin but could not serve with it" >&2
    exit 1
  fi

  stop_serve
  echo "  PASS: $RUNTIME runs the packed bin from under node_modules (version $ACTUAL) and serves the strip on :$PORT"
  RAN=$((RAN + 1))
done

if [ "$RAN" -eq 0 ]; then
  echo "  FAIL: no runtime was available to run the packed bin" >&2
  exit 1
fi
echo "\n=== All shell scripts have been executed successfully ==="
