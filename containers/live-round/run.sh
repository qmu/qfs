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

echo ">> generating minimal credential (claudeAiOauth only; mcpOAuth excluded)"
CRED=$(sh "$HERE/extract-min-cred.sh")
trap 'rm -f "$CRED"' EXIT INT TERM

echo ">> building $IMAGE"
"$ENGINE" build -t "$IMAGE" -f "$HERE/Containerfile" "$HERE"

# Common, safety-checked mount set. NOTE the credential is mounted at the agent's HOME
# path directly — the host ~/.claude DIRECTORY is never mounted.
set_args() {
  MOUNTS="--rm \
    --userns=keep-id \
    -v $WORKTREE:/src:ro \
    -v $CRED:/home/agent/.claude/.credentials.json:ro \
    --mount type=volume,dst=/work \
    -w /work"
}
set_args

ROUND="${1:-}"
if [ -n "$ROUND" ] && [ -f "$HERE/$ROUND" ]; then
  echo ">> running leg script $ROUND in the box"
  # shellcheck disable=SC2086
  exec "$ENGINE" run -i $MOUNTS -v "$HERE":/round:ro "$IMAGE" sh "/round/$ROUND"
fi

echo ">> entering container: source /src (ro), scratch /work, HOME /home/agent (fresh)"
echo "   legs (per the ticket): build release binary; capture the teams-inbox message schema;"
echo "   wire+test steering; steering live fire on a container-local session; launch live fire"
echo "   (real 'claude --bg'); composed launch->steer proof. Any leg that cannot run in-container"
echo "   is recorded 'blocked' with the missing piece — never waits, never escalates overnight."
# shellcheck disable=SC2086
exec "$ENGINE" run -it $MOUNTS "$IMAGE"
