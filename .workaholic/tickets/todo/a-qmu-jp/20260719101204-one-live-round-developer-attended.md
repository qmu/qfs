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

# One live round, developer-attended

Satisfies mission acceptance: **"One live round, developer-attended."** After the seam ticket
lands, exercise the shipped path end to end: a real request carrying a session and a real request
without one, each resolving to its answer, with output and raw exit codes pasted.

## DEVELOPER-ATTENDED — cannot be completed by an unattended agent

The mission gate and this acceptance item both say **developer-attended**. An unattended
`/monitor` leaf cannot paste a developer-observed live round. This ticket is therefore driven by
the developer (or a developer-attended `/drive`), NOT autonomously.

## Steps (for the attended run)

1. Build the release binary from this branch.
2. Anonymous path: `qfs run -e '/sys/whoami'` (or the HTTP query face with no session cookie) —
   expect `signed_in=false, user=null`, exit 0. Paste output + exit code.
3. Session path: mint a session through the OAuth sign-in face (`qfs/src/oauth.rs`), then issue a
   query request carrying that session cookie — expect `signed_in=true, user=<id>`. Paste output
   + exit code.
4. Policy both-directions (optional but recommended): with a `FOR <user>` narrowed rule bound,
   show it bites under the session and contributes nothing anonymous.

## Policies

**運用 / `workaholic:operation`**
- `ci-cd` / ship-on-real-response — "Ground the decision to ship not in the fact that the process
  turned green but in the fact that production actually responds as expected." The live round is
  that ground for this mission.

**設計 / `workaholic:design`**
- `access-control` — the round proves resolution happens under the caller's principal on the real
  path, not only in tests.

## Quality Gate

**Acceptance criteria.** Two real requests (session + no session) resolve to their respective
answers through the shipped path; output and raw exit codes are pasted into the ticket/PR.

**Verification method.** Developer runs the two commands and records the transcript.

**Gate that must pass.** The transcript shows the correct answers and exit codes; the branch
gates (build/test/clippy/fmt/xtask) already green from the seam ticket.
