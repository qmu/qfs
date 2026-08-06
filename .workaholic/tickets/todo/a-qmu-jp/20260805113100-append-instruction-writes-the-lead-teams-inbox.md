---
created_at: 2026-08-05T11:31:00+09:00
author: a@qmu.jp
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
