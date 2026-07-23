# who-am-I live-round container

Turnkey isolated box for ticket `20260719101204-one-live-round-developer-attended.md`
(mission `a-request-resolves-to-a-principal-the-query-path-can-read`). Re-ruled to run
autonomously in a container, transcript reviewed in the morning.

## Run

```sh
sh containers/live-round/run.sh            # interactive
# or
sh containers/live-round/run.sh whoami.sh  # if you drop a whoami.sh round script here
```

## What it proves

1. **Anonymous** — `qfs run -e '/sys/whoami'` with no session → `signed_in=false`, `user=null`, exit 0.
2. **Session-carrying** — seed the local identity store (invite redeem), mint a session through
   the local OAuth sign-in face (`qfs/src/oauth.rs`), issue a query carrying the session cookie →
   `signed_in=true`, `user=<id>`.
3. Optionally, a `FOR <user>` rule bites with the session and contributes nothing anonymously.

The **deliverable is the transcript**: every command + verbatim stdout/stderr + raw exit codes
for both the session and no-session cases, pasted into the ticket Final Report / PR story.

## Safety

- No credentials, no cloud, no network to any private data — the round is entirely local to the
  container (qfs identity store + localhost OAuth face). The worktree is mounted read-only.
- If step 2's session mint cannot be stood up in-container (the untested drop-in the earlier
  unattended leaf blocked on), record it **`blocked`** with the exact missing piece and continue —
  never wait for or escalate to the developer overnight.
