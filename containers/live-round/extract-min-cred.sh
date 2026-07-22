#!/bin/sh -eu
# Generate a MINIMAL Claude credential for the live-round container.
#
# The host ~/.claude/.credentials.json holds two things:
#   - claudeAiOauth : the token the Claude Code CLI needs to authenticate. REQUIRED.
#   - mcpOAuth      : OAuth tokens for CONNECTED SERVICES (Gmail, Drive, Slack, ...). These
#                     must NEVER enter the container — they are the user's live cloud data.
#
# This writes a fresh credentials file containing ONLY claudeAiOauth, mode 0600, to a path it
# prints on stdout. run.sh mounts THAT file (read-only) into the container — the host ~/.claude
# directory is never mounted, and mcpOAuth never leaves the host.
#
# Usage: extract-min-cred.sh [output-path]   (default: a private temp file)

set -eu

SRC="${CLAUDE_CREDENTIALS:-$HOME/.claude/.credentials.json}"
OUT="${1:-${TMPDIR:-/tmp}/qfs-liveround-cred.$$.json}"

[ -f "$SRC" ] || { echo "no credentials at $SRC" >&2; exit 1; }

umask 077
python3 - "$SRC" "$OUT" <<'PY'
import json, sys
src, out = sys.argv[1], sys.argv[2]
with open(src) as f:
    d = json.load(f)
minimal = {}
if "claudeAiOauth" in d:
    minimal["claudeAiOauth"] = d["claudeAiOauth"]
if not minimal:
    sys.exit("credentials file has no claudeAiOauth key — cannot authenticate the CLI")
with open(out, "w") as f:
    json.dump(minimal, f)
PY
chmod 600 "$OUT"
echo "$OUT"
