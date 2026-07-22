---
created_at: 2026-07-19T10:12:04+09:00
author: a@qmu.jp
type: housekeeping
layer: [Infrastructure]
effort: 1h
commit_hash:
category: Changed
depends_on:
mission: a-request-resolves-to-a-principal-the-query-path-can-read
---

# One live round — autonomous, in an isolated container, transcript recorded

Satisfies mission acceptance item 8. **Re-ruled by the owner, 2026-07-22** (overnight-run
directive: nothing in the night's queue waits for the developer): the round runs
**autonomously inside a container** (`docker` on this host is podman 5.8.4), and the
developer reviews the **recorded transcript** in the morning instead of attending the run.
The original developer-attended framing is superseded by this ruling; the evidence bar is
unchanged — raw output and raw exit codes, pasted.

## Steps (autonomous, in-container)

Run every step inside a fresh container (no host `~/.claude`, no host sockets; the repo
mounted or copied in is fine — this round touches qfs only, not Claude Code):

1. Build the release binary from this branch.
2. Anonymous path: `qfs run -e '/sys/whoami'` (or the HTTP query face with no session
   cookie) — expect the explicit not-signed-in answer (`signed_in=false, user=null`-shaped),
   exit 0. Record output + exit code verbatim.
3. Session path: seed the identity store (invite redeem or the test seam the branch
   provides), mint a session through the OAuth sign-in face (`qfs/src/oauth.rs`), then issue
   a query request carrying that session cookie — expect the named principal
   (`signed_in=true, user=<id>`). Record output + exit code verbatim.
4. Policy both-directions (recommended): with a `FOR <user>` narrowed rule bound, show it
   bites under the session and contributes nothing anonymous. Record both transcripts.
5. Paste the full transcript (commands, outputs, exit codes) into this ticket's Final Report
   and the PR story — that transcript is the deliverable the developer reviews.

If a step cannot be completed in-container, record it `blocked` with the exact missing piece
— do not run it on the shared host, and do not wait for the developer mid-run.

## Policies

**運用 / `workaholic:operation`**
- `ci-cd` / ship-on-real-response — "Ground the decision to ship not in the fact that the
  process turned green but in the fact that production actually responds as expected." The
  recorded live round is that ground for this mission; the developer's looking-through
  relocates to the morning transcript review (owner re-ruling, 2026-07-22).

**設計 / `workaholic:design`**
- `access-control` — the round proves resolution happens under the caller's principal on the
  real path, not only in tests.

**安全**
- Isolated container only; never mount the host `~/.claude` or its sockets. This round needs
  neither — it exercises qfs alone.

## Quality Gate

**Acceptance criteria.** Two real requests (session + no session) resolve to their
respective answers through the shipped path; the raw transcript (output + exit codes) is
pasted into the ticket Final Report and PR.

**Verification method.** The autonomous leaf runs the commands in the container and records
the transcript verbatim; the developer reviews it in the morning.

**Gate that must pass.** The transcript shows the correct answers and exit codes; the branch
gates (build/test/clippy/fmt/xtask) green.
