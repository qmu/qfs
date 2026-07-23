# Claude-session live-round container

Turnkey isolated box for the mission's live legs — ticket
`20260719231005-claude-live-round-owner-attended.md` (re-ruled autonomous-in-container), which
is the live-fire vehicle for items 5 & 6 (`20260717010500` steering, `20260717010600` launch),
and the sanctioned home for `20260719105527`'s real-tmux-server test.

## Why a container is mandatory

Spawning / killing / steering a real `claude` process, or tearing down a real tmux server, on the
shared **host** has repeatedly crashed the parent and sibling live sessions. Every such action
runs **only** inside this box, which has a fresh `$HOME`, its own tmux socket, and no host
`~/.claude`, no host tmux socket, and no host qfs socket.

## Run

```sh
sh containers/live-round/run.sh            # interactive
# or
sh containers/live-round/run.sh round.sh   # run a leg script dropped in this dir
```

## Credentials — minimal by construction

`extract-min-cred.sh` writes a fresh `.credentials.json` containing **only** `claudeAiOauth` (the
CLI auth token), dropping `mcpOAuth` — the connected-service (Gmail/Drive/Slack) OAuth that must
never enter the container. `run.sh` mounts **that generated file** read-only at the agent's HOME;
the host `~/.claude` directory is never mounted, and the token is never baked into an image layer.

## Legs the overnight leaf runs (per the ticket)

1. Build the release binary from this branch.
2. Capture the teams-inbox message-object schema from a real in-flight message (until captured,
   steering stays **fail-closed** — do not wire a guessed schema).
3. Wire + test steering (hermetic append behind a fake inbox is safe anywhere; the live drain is here).
4. Steering live fire against a container-local session.
5. Launch live fire — a real `claude --bg` (irreversible-gated; flat-rate, but a real actor).
6. Composed launch → steer proof.

Record every command + verbatim stdout/stderr + raw exit codes as the transcript for morning review.

## Known blockers (record `blocked`, never wait)

- In-container Claude CLI auth from the copied minimal credential is **unverified** — if it fails,
  legs 4–6 go `blocked` with that exact reason and the run continues.
- If the CLI failed to install at image-build time, `podman cp` the host binary in (see `run.sh`
  header; host is aarch64 and the base image matches, so the native binary runs).
