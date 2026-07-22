---
created_at: 2026-07-19T23:10:05+09:00
author: a@qmu.jp
type: housekeeping
layer: [Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on: [20260717010500-claude-steering-rewire.md, 20260717010600-claude-session-create-launch.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# One live round for /claude — autonomous, in an isolated container

Consolidates the remaining **live proofs** for mission acceptance items 5 (steering) and 6
(launch). The hermetic slices are already done or specified: launch's hermetic implementation
shipped (commit `a73fa01`, `v0.0.81`); the steering applier stays fail-closed pending the
inbox-schema capture below.

**Re-ruled by the owner, 2026-07-22** (mission Scope, container ruling): a **container on this
host is the sanctioned isolated box**, and this round runs **autonomously inside it** — the
overnight run does not wait for the owner; the deliverable is the recorded transcript. The
shared-host prohibition is unchanged and absolute: no spawn, kill, steer, or tmux exercise
against the host's live sessions, ever.

## Container contract

- Fresh container (`docker` = podman 5.8.4), repo copied/mounted in, rust toolchain available.
- **Never mount the host `~/.claude`, its teams/session state, or any tmux socket.** The
  container gets a fresh `$HOME`.
- To run a real `claude` in-container, install the Claude Code CLI in the image and copy in
  the **minimal auth credential only** (decision 2026-07-22, veto-able: credentials file only
  — never the host's sessions/, teams/, projects/ state). The credential dies with the
  container.
- Every process this round spawns (claude sessions, tmux servers if any) lives and dies
  inside the container; teardown targets only container-local names.

## Steps (autonomous, in-container)

1. **Build** the release binary from this branch.
2. **Capture the teams-inbox message-object schema (item 5's single unknown).** Two sanctioned
   routes, in order: (a) read-only inspection of an in-flight inbox JSON — including on the
   host store, which is safe because it is a file read, never a process interaction; (b) start
   a container-local session and make it produce an inbox message, then snapshot it. Record
   the element field names verbatim.
3. **Wire + hermetically test steering** against the captured schema: `append_instruction`
   appends one message object to the target's teams-inbox array behind the unchanged
   `SessionSource` seam; `scan_instructions` reads it back; unknown / non-team session fails
   closed; UPDATE/REMOVE still structurally rejected.
4. **Steering live fire (item 5).** In-container: INSERT into
   `/hosts/local/claude/sessions/<id>/instructions` for a real container-local session,
   observably drained. Record command, output, exit code.
5. **Launch live fire (item 6).** In-container: the launch INSERT with
   `--commit --commit-irreversible` spawns a real `claude --bg`; the returned id appears in
   the sessions relation. Also confirm `--commit` alone fails closed
   (`irreversible_ack_required`). Record outputs + exit codes.
6. **Composed proof.** Launch a scratch session, read its id back, steer it via its inbox —
   launch → row visible → steerable, end to end, all in-container.

**Fallback contract (run through, never wait):** if a leg cannot run in-container (e.g. the
CLI cannot authenticate), record that leg `blocked` with the exact missing piece and complete
every other leg — never run a live leg on the shared host, and never stop the night to ask.

## Policies

**運用 / `workaholic:operation`**
- `ci-cd` / ship-on-real-response — ground the ship in the system actually responding; this
  recorded round is that ground for items 5 and 6, reviewed by the developer in the morning.

**設計 / `workaholic:design`**
- `access-control` — steering and launch run as the operator, same-user/same-host only.

**安全 / safety floor (mission ABSOLUTE prohibition)**
- No real spawn/kill/live-steer on the shared host; container only; no shell interpolation
  ever (`Command::new(<configured binary>)` with cwd/prompt/name as arguments); teardown by
  container-local target only, never a bare `kill-server`.

## Quality Gate

**Acceptance criteria.** The steering message schema is captured and the applier wired+tested
against it; a real INSERT steers a real container-local session (observably drained); a real
launch INSERT spawns `claude --bg` in-container and the new id appears in the sessions
relation; the composed launch→steer round is demonstrated. Raw output and exit codes pasted
into the ticket Final Report / PR; any in-container-impossible leg recorded `blocked` with its
named missing piece.

**Verification method.** The autonomous leaf runs the round in the container and records the
transcript verbatim; the developer reviews it in the morning.

**Gate that must pass.** The transcript shows the correct behaviour and exit codes; the branch
gates (build/test/clippy/fmt/xtask) green.
