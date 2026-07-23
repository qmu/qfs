#!/bin/sh -eu
# Build and enter the Claude-session live-round container (ticket 20260719231005 and the
# live-fire legs of 20260717010500 / 20260717010600, plus 20260719105527's real tmux test).
#
# What this does:
#   1. Builds the rust + Claude-CLI + tmux image (Containerfile in this dir).
#   2. Generates a MINIMAL credential (only claudeAiOauth — see extract-min-cred.sh) so the
#      CLI can authenticate WITHOUT the host's connected-service (mcpOAuth) tokens.
#   3. Runs the box with: this worktree :ro at /src, an auto-removed /work volume, a FRESH
#      $HOME, and the minimal credential mounted read-only. Nothing host-sensitive is mounted.
#
# ABSOLUTE safety invariants (do not weaken — these prevent crashing host sessions):
#   - host ~/.claude is NEVER mounted; only the generated minimal credential file is.
#   - host tmux socket / TMUX env is NEVER passed in; the box has its own TMUX_TMPDIR + a
#     dedicated -L socket, so a stray kill-server can only ever hit the container's own server.
#   - the worktree is :ro; no host qfs socket, no host cloud state is present.
#   - every `claude` spawn / steer / kill and every real tmux teardown happens ONLY in here.
#
# Usage:
#   sh containers/live-round/run.sh             # interactive shell in the box
#   sh containers/live-round/run.sh round.sh    # run ./round.sh (a leg script) in the box
#
# Fallback if the CLI failed to install at build time: start the box, then from the HOST
#   podman cp "$(command -v claude)" <container>:/home/agent/.local/bin/claude
# (host is aarch64; the base image is the matching arch, so the native binary runs.)

set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORKTREE=$(CDPATH= cd -- "$HERE/../.." && pwd)
IMAGE="qfs-claude-liveround"
ENGINE="${CONTAINER_ENGINE:-podman}"

# The credential is generated into a private DIRECTORY (not a bare file). We mount that dir at
# /cred:ro and COPY it into a container-owned ~/.claude at start-up — a direct bind-mount at
# ~/.claude/.credentials.json makes ~/.claude root-owned under --userns=keep-id, and the real
# `claude` then fails `EACCES` trying to mkdir ~/.claude/jobs (proven 2026-07-22). The host
# ~/.claude DIRECTORY is still never mounted; only the extracted claudeAiOauth crosses.
echo ">> generating minimal credential (claudeAiOauth only; mcpOAuth excluded)"
CREDDIR=$(mktemp -d)
sh "$HERE/extract-min-cred.sh" "$CREDDIR/.credentials.json" >/dev/null
trap 'rm -rf "$CREDDIR"' EXIT INT TERM

# The Containerfile's `curl claude.com/install.sh` currently 404s, so the CLI is usually NOT baked
# into the image. Mount the host's self-contained `claude` binary read-only instead (proven route:
# it is a static aarch64 ELF needing only glibc, which the base image has; auth from the minimal
# credential succeeds). Host is aarch64 and the base image matches, so the native binary runs.
HOSTCLAUDE=$(readlink -f "$(command -v claude 2>/dev/null || true)" 2>/dev/null || true)
CLAUDE_MOUNT=""
if [ -n "$HOSTCLAUDE" ] && [ -x "$HOSTCLAUDE" ]; then
  echo ">> mounting host claude binary read-only: $HOSTCLAUDE -> /home/agent/.local/bin/claude"
  CLAUDE_MOUNT="-v $HOSTCLAUDE:/home/agent/.local/bin/claude:ro"
else
  echo ">> WARNING: no host claude binary found; relying on the image's own (install.sh may have 404'd)"
fi

echo ">> building $IMAGE"
"$ENGINE" build -t "$IMAGE" -f "$HERE/Containerfile" "$HERE"

# Common, safety-checked mount set. The env vars point qfs at the in-container claude + the
# writable ~/.claude the bootstrap sets up below.
set_args() {
  MOUNTS="--rm \
    --userns=keep-id \
    -v $WORKTREE:/src:ro \
    -v $CREDDIR:/cred:ro \
    $CLAUDE_MOUNT \
    -e QFS_CLAUDE_BINARY=/home/agent/.local/bin/claude \
    -e QFS_CLAUDE_SESSIONS=/home/agent/.claude \
    --mount type=volume,dst=/work \
    -w /work"
}
set_args

# Bootstrap run inside the box before any leg/shell: materialise a WRITABLE ~/.claude from the
# read-only /cred mount so the real `claude` can create its own jobs/sessions dirs.
BOOT='mkdir -p "$HOME/.claude" && cp /cred/.credentials.json "$HOME/.claude/.credentials.json" && chmod 600 "$HOME/.claude/.credentials.json"'

ROUND="${1:-}"
if [ -n "$ROUND" ] && [ -f "$HERE/$ROUND" ]; then
  echo ">> running leg script $ROUND in the box"
  # shellcheck disable=SC2086
  exec "$ENGINE" run -i $MOUNTS -v "$HERE":/round:ro --entrypoint /bin/sh "$IMAGE" \
    -c "$BOOT && exec sh \"/round/$ROUND\""
fi

echo ">> entering container: source /src (ro), scratch /work, HOME /home/agent (fresh)"
echo "   legs (per the ticket): build release binary; capture the teams-inbox message schema;"
echo "   wire+test steering; steering live fire on a container-local session; launch live fire"
echo "   (real 'claude --bg'); composed launch->steer proof. Any leg that cannot run in-container"
echo "   is recorded 'blocked' with the missing piece — never waits, never escalates overnight."
# shellcheck disable=SC2086
exec "$ENGINE" run -it $MOUNTS --entrypoint /bin/sh "$IMAGE" -c "$BOOT && exec sh -i"
