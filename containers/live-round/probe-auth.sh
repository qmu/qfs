#!/bin/sh -eu
# Live-round leg 0: probe whether the Claude CLI can AUTHENTICATE in-container from the copied
# minimal credential — the ticket's flagged unknown (README "Known blockers"). This gates legs 2b–6
# (schema capture, steering live fire, launch live fire, composed proof): if auth fails here, those
# legs are recorded `blocked` with this exact output and the run does NOT fall back to the host.
#
# Cheapest real probe: a one-shot non-interactive `claude -p` turn. It never launches a persistent
# session and stays entirely inside the container.
set -eu

echo "== identity / home =="
id
echo "HOME=$HOME"

echo "== claude CLI present in image? =="
if ! command -v claude >/dev/null 2>&1; then
  echo "RESULT: BLOCKED — claude CLI not installed in image (install.sh failed at build; podman cp fallback needed)"
  exit 0
fi
claude --version 2>&1 | head -3 || echo "(claude --version returned nonzero)"

echo "== credential mounted (claudeAiOauth only)? =="
if [ -f "$HOME/.claude/.credentials.json" ]; then
  echo "credential file present ($(wc -c < "$HOME/.claude/.credentials.json") bytes)"
else
  echo "RESULT: BLOCKED — no credential mounted at \$HOME/.claude/.credentials.json"
  exit 0
fi

echo "== non-interactive auth probe: claude -p =="
set +e
OUT=$(timeout 120 claude -p 'Reply with exactly the word: ok' 2>&1)
RC=$?
set -e
echo "---- claude -p output (rc=$RC) ----"
printf '%s\n' "$OUT" | head -40
echo "---- end output ----"
if [ "$RC" -eq 0 ]; then
  echo "RESULT: AUTH_OK — the CLI authenticated in-container; live legs can proceed"
else
  echo "RESULT: BLOCKED — claude -p exited $RC (auth from the minimal credential did not succeed in-container)"
fi
