#!/bin/sh -eu
# Build and enter the who-am-I live-round container (ticket 20260719101204).
#
# What this does:
#   1. Builds the rust image (Containerfile in this dir).
#   2. Runs it with THIS worktree bind-mounted READ-ONLY at /src and an isolated,
#      auto-removed writable volume at /work (cargo target + TMPDIR live here — never on
#      the host tree, never on host /tmp).
#   3. Either drops you into a shell, or runs a round script from this dir if named.
#
# Safety invariants (do not weaken):
#   - The worktree is mounted :ro — the round cannot mutate the branch. Local
#     identity-store / session writes happen under /work and die with the container.
#   - NO credentials, NO host ~/.claude, NO host sockets are mounted. This round needs none.
#   - --network is the engine default: cargo fetches crates, but no cloud credential
#     exists in this box, so network access reaches no private data.
#
# Usage:
#   sh containers/live-round/run.sh              # interactive shell in the box
#   sh containers/live-round/run.sh whoami.sh    # run ./whoami.sh (a round script) in the box
#
# Inside the box, the ticket's steps are:
#   cp -r /src/. /work/build && cd /work/build
#   cargo build --release -p qfs
#   ./target/release/qfs run -e '/sys/whoami'            # anonymous -> signed_in=false
#   # then mint a session via the local OAuth sign-in face and re-issue with the cookie
#   #  -> signed_in=true. Capture every command + verbatim stdout/stderr + raw exit code.

set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORKTREE=$(CDPATH= cd -- "$HERE/../.." && pwd)
IMAGE="qfs-whoami-liveround"
ENGINE="${CONTAINER_ENGINE:-podman}"

echo ">> building $IMAGE"
"$ENGINE" build -t "$IMAGE" -f "$HERE/Containerfile" "$HERE"

ROUND="${1:-}"
if [ -n "$ROUND" ] && [ -f "$HERE/$ROUND" ]; then
  echo ">> running round script $ROUND in the box"
  exec "$ENGINE" run --rm -i \
    --userns=keep-id \
    -v "$WORKTREE":/src:ro \
    -v "$HERE":/round:ro \
    --mount type=volume,dst=/work \
    -w /work \
    "$IMAGE" "/round/$ROUND"
fi

echo ">> entering container: source at /src (ro), scratch at /work"
exec "$ENGINE" run --rm -it \
  --userns=keep-id \
  -v "$WORKTREE":/src:ro \
  --mount type=volume,dst=/work \
  -w /work \
  "$IMAGE"
