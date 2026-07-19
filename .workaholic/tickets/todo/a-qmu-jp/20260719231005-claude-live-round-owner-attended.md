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

# One live round for /claude — owner-attended, isolated environment only

Consolidates the remaining **live proofs** for mission acceptance items 5 (steering) and 6
(launch) into a single owner-attended round. The hermetic slices are already done or specified:
launch's hermetic implementation shipped (commit `a73fa01`, `v0.0.81` — INSERT grammar, `Sessions`
widened Select→Select+Insert, irreversible gate, launcher seam behind a fake, hermetic tests); the
steering applier stays fail-closed pending the one owner-attended input this ticket captures. What
is left is exactly the set of steps that spawn or observe **real** Claude Code processes — none of
which an unattended agent may perform on this shared host.

## OWNER-ATTENDED, ISOLATED ENVIRONMENT — cannot be completed by an unattended agent

**Absolute prohibition (owner rulings 2026-07-19, mission Scope environment constraint).** Every
step below touches real OS processes: launch spawns a real `claude --bg`, and capturing/observing a
real session's teams inbox reads a live session's state. On this shared host — which runs the
owner's live Claude Code sessions, including the parent of any `/monitor` run — exercising these
legs has repeatedly crashed the parent and sibling sessions. Therefore this ticket:

- is driven by the **developer in an owner-attended session**, NEVER autonomously / by `/monitor`;
- runs ONLY in an **isolated box** (a container/VM with no live Claude Code sessions), never the
  shared host;
- stays in `todo/`, **NOT drive-authorized**, until an attended run in an isolated environment.

## Steps (for the attended, isolated run)

1. **Build** the release binary from this branch in the isolated box.
2. **Capture the teams-inbox message-object schema (steering, item 5).** Snapshot a real
   `~/.claude/teams/<session>/inboxes/<member>.json` the moment a message is in flight (every
   observed inbox drains to `[]`), OR read the shape from the CLI's own inbox-writer. Record the
   element field names (`from`/`to`/`text`/`id`/`ts` or whatever the product actually writes). This
   is the single unknown blocking the steering applier — the append target (per-recipient JSON
   array) is already known; only the element schema is not.
3. **Wire + hermetically test steering** against the captured schema: `append_instruction` appends
   one message object to the target's teams-inbox array behind the unchanged `SessionSource`
   seam; `scan_instructions` reads the same array back; unknown / non-team session fails closed;
   UPDATE/REMOVE still structurally rejected. (This wiring MAY be authored anywhere; it is gated
   here only because it is meaningless without the step-2 capture.)
4. **Steering live fire (item 5 QG1).** An INSERT into
   `/hosts/local/claude/sessions/<id>/instructions` for a real target session is observably drained
   by that session. Paste the command, output, and raw exit code.
5. **Launch live fire (item 6 QG2).** `insert into /hosts/local/claude/sessions values (cwd, prompt,
   name) ('<dir>','<prompt>','<name>') returning id` with `--commit --commit-irreversible` spawns a
   real `claude --bg`; the returned id then appears in `/hosts/local/claude/sessions`. Paste
   output + exit code. Confirm `--commit` WITHOUT `--commit-irreversible` fails closed
   (`irreversible_ack_required`).
6. **Composed proof (items 5+6).** Launch a scratch session, read its id back from the sessions
   relation, then steer it via its teams inbox — launch → row visible → steerable, end to end.

## Policies

**運用 / `workaholic:operation`**
- `ci-cd` / ship-on-real-response — ground the ship in production actually responding as expected,
  not in a green process. This round is that ground for items 5 and 6.

**設計 / `workaholic:design`**
- `access-control` — steering and launch run as the operator, same-user/same-host only; the round
  proves it on the real path, not only in tests.

**安全 / safety floor (mission ABSOLUTE prohibition)**
- No real spawn/kill/live-steer on the shared host; isolated environment only; no shell
  interpolation ever (`Command::new(<configured binary>)` with cwd/prompt/name as arguments).

## Quality Gate

**Acceptance criteria.** The steering message schema is captured and the applier wired+tested
against it; a real INSERT steers a real target session (observably drained); a real launch INSERT
spawns `claude --bg` and the new id appears in the sessions relation; the composed launch→steer
round is demonstrated. Output and raw exit codes pasted into the ticket/PR.

**Verification method.** Developer runs the commands in an isolated box and records the transcript.

**Gate that must pass.** The transcript shows the correct behaviour and exit codes; the branch
gates (build/test/clippy/fmt/xtask) green.
