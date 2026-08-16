---
created_at: 2026-08-05T11:31:00+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260805113000-capture-the-teams-inbox-contract-in-a-container.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
merge_policy: review
---

# append_instruction writes the target session's lead teams inbox, or fails closed saying why

## Overview

`SessionSource::append_instruction` currently fails closed with a structured error for every write
(`packages/qfs/crates/qfs/src/claude.rs:45-48`). That refusal is deliberate and correct — the
retired layout appended to a file nothing read, and an honest refusal beats a write-only no-op — but
it means acceptance item 5 is unmet: steering does not reach a session.

This ticket makes the write real, against the contract the preceding spike captured.

**Addressing is lead-fixed (developer ruling, 2026-08-05).** A session id identifies a session, and
`config.json`'s `leadSessionId` is that session's own id, so an INSERT into
`/hosts/<host>/claude/sessions/<id>/instructions` appends to the **lead member's** inbox. Members
are not addressable: no `.../members/<member>/instructions` path is added here. The alternative —
making members addressable — was weighed and declined for now because it widens the path grammar
before anything has asked for it; it stays available as a later addition, which is the cheap
direction.

## Scope

**In scope.**

1. Resolve a session id to its lead inbox path: session UUID → `~/.claude/teams/session-<first-8>/`
   → `config.json` → the member whose `agentId` matches `leadSessionId`'s lead → its
   `inboxes/<member>.json`. Use the mapping the spike confirmed, not the one assumed here.
2. Append one well-formed message to that JSON array, in the schema the spike captured. The append
   must be **atomic against a concurrent drain** in whatever way the spike's drain-semantics finding
   requires (read-modify-write under a lock, or write-new-then-rename — the finding decides).
3. **Fail closed, loudly, when the target has no inbox.** A structured error naming what is missing
   and why — not a silent success, not a created directory. Creating an inbox is only permitted if
   the spike proved an unsolicited inbox is actually drained; if it proved otherwise, the refusal is
   the correct behaviour and the error message says so.
4. Keep the change **behind the `SessionSource` seam**, in `qfs/src/claude.rs`. The
   `qfs-driver-claude` crate stays credential-free and I/O-free; its pure half is not the place for
   filesystem knowledge.
5. Update the module doc comment that currently states steering is not wired.

**Out of scope.** The live-fire proof (its own ticket). Any change to the read path, the schema
columns, or the launch surface. Making members addressable.

## Policies

- workaholic:implementation / honest-surfaces — after this lands, a successful INSERT must mean the
  message is in a queue a session drains. If it cannot be, the write must fail. The one thing this
  ticket may not produce is a write that reports success and delivers nothing, which is exactly the
  state that was torn out.
- workaholic:design / 「推測するな、宣言して拒否せよ」 — a session with no inbox is refused with a
  named reason, never guessed at by inventing the directory.
- workaholic:implementation / domain-layer-separation — the driver crate's pure half stays pure; the
  filesystem work lives behind the seam in the binary, as the existing reader does.

## Quality Gate

1. A hermetic test over a fixture team directory asserts an INSERT appends exactly one message, in
   the captured schema, to the **lead** member's inbox, leaving other members' inboxes untouched.
2. A hermetic test asserts a session with a `config.json` but no `inboxes/` is refused with a
   structured error whose reason names the missing inbox — and that nothing is created on disk.
3. A hermetic test asserts a concurrent drain (the file emptied to `[]` between read and write) does
   not lose the appended message, per the spike's drain-semantics finding.
4. UPDATE and REMOVE on the instructions path remain rejected — the existing double rejection
   (`lib.rs:91-98`, `applier.rs:62-66`) still holds.
5. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets --
   -D warnings`, `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check`.

## Considerations

- Everything here is hermetic — fixture directories, no real session — so it may be authored
  anywhere. Only the next ticket needs a container.
- If the spike found the message schema carries a sender identity, decide what qfs puts there and
  record it: an honest "written by qfs on behalf of the operator" is better than impersonating a
  team member that does not exist.

## Final Report

Development completed. Steering is real: `SessionSource::append_instruction` no longer fails closed.

**The medium is not the one this ticket's title names, and that substitution is the ticket's own
instruction.** Scope item 1 says "Use the mapping the spike confirmed, not the one assumed here".
The spike (`20260805113000`) confirmed that a solo session has no teams inbox and never drains one,
and that the medium a live session reads is its **peer-messaging Unix domain socket** — path and
token both in the `sessions/<pid>.json` record the reader already parses. So the implementation
resolves a session id to that socket, writes the two-line protocol (auth, then a user message), and
returns 1.

**No developer ruling was needed.** The spike's Considerations foresaw one *if* the answer narrowed
acceptance item 5 to team-formed sessions. It does not: the peer socket steers **any** live session,
solo included, so item 5's wording holds unchanged and is met rather than narrowed.

**Fail-closed, with a named reason each time** — unknown session, a leftover record of a dead
process, a record publishing no `messagingSocketPath`, no readable peer token, a refused connection.
No path is invented and no directory is created; the token is never echoed into an error. A
malformed payload is rejected before any session is addressed, so a blank steer never opens a socket.
The instructions log still reads back empty: a socket carries no queryable backlog.

**Quality Gate items 1–3 were written for a file medium and were translated, not dropped** (the
translation is recorded in the design brief so the substitution is visible):

1. *one message to the lead inbox, other members untouched* → `steering_delivers_one_authenticated_
   message_to_the_addressed_session`: exactly two protocol lines with the record's own token reach
   the addressed session's socket, and a second live session's listener has no pending connection.
2. *no inbox ⇒ structured refusal, nothing created on disk* →
   `steering_refuses_a_session_that_publishes_no_socket`, which also asserts the sessions directory
   is unchanged and no `teams/` directory appears. Plus `steering_refuses_when_no_peer_token_is_
   readable`, `steering_refuses_an_unknown_session`, `steering_refuses_a_leftover_record`.
3. *a concurrent drain must not lose the append* → `concurrent_steers_do_not_lose_each_other`: the
   socket is a stream, not a read-modify-write file, so the lost-update class cannot arise; two
   concurrent appends both arrive.

Item 4 holds unchanged (the UPDATE/REMOVE double rejection is untouched). Item 5: `cargo test
--workspace` green, `clippy --workspace --all-targets -- -D warnings` clean, `fmt --check` clean,
`gen-docs --check` / `gen-skills --check` / `check-migrations` all in sync.

One extra test beyond the gate: `a_multiline_instruction_cannot_forge_a_protocol_line` — instruction
text is data, so a payload full of newlines and JSON still crosses as one string value.

### Discovered Insights

- **Insight**: The `qfs-driver-claude` / binary split paid off exactly as designed. Changing the
  steering medium from a file format to a socket protocol touched one function behind the
  `SessionSource` seam; the driver crate, its capability gates, its irreversibility classification
  and every test in it were untouched.
  **Context**: The seam was justified on wasm-buildability and dep direction. Its real return was
  absorbing a total change of external mechanism without moving a boundary.

- **Insight**: `teams_inbox.rs` is now dead code that must never be wired — a complete, tested
  primitive built against an assumption the spike disproved. Its doc comment carries the disproof;
  whether to delete it is the developer's call.
  **Context**: It is the concrete cost of building the transport before capturing the contract — the
  same order of operations the retired pty/tmux transport was retired for.
