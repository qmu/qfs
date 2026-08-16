---
created_at: 2026-08-16T16:11:43+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
merge_policy: review
verification_handoff:
---

# `last_message` reads null for a busy session, so the mission's own gate cannot pass

## Overview

The mission's `gate_assert` requires every row of `/claude/sessions` to carry a **non-empty**
`last_message`. Run against this container's real store on 2026-08-16, the endpoint answered `200`
with the driving session's row present and `last_message: null`:

```
$ curl http://127.0.0.1:8787/claude/sessions
{"rows":[{"id":"cdb3f812-…","cwd":"/home/user/qfs","name":"qfs-5c",
          "status":"unknown","last_message":null}], …}
```

The cause is measured, not guessed. `last_visible_message` reads only the transcript's last
`TRANSCRIPT_TAIL_BYTES` (256 KiB) and walks back for a `user`/`assistant` entry with visible text.
For that session's 1 609 436-byte transcript:

- entries inside the 256 KiB tail: **109**
- of those, with visible text: **0**
- with visible text in the whole file: **4**

An agentic session is almost all tool traffic, and `tool_use` / `tool_result` deliberately never
surface (that redaction is correct and must stay). So the window is the defect: the comment claims
"the last visible text virtually always lives in the final few entries", and for this workload it
lives ~1.3 MB back.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — all code work.
- `workaholic:implementation` / `policies/directory-structure.md` — all code work.
- `workaholic:implementation` / `honest-surfaces` — a column that is null because the reader stopped
  looking, not because there is nothing to show, reports absence it did not establish.

## Key Files

- `packages/qfs/crates/qfs/src/claude.rs` — `TRANSCRIPT_TAIL_BYTES`, `last_visible_message`,
  `read_tail`, `entry_visible_text`.
- `.workaholic/missions/active/claude-code-sessions-are-queryable-and-steerable-as-qfs-paths/mission.md`
  — the `gate_assert` this blocks.

## Implementation Steps

1. Reproduce with a fixture transcript whose 256 KiB tail is pure tool traffic and whose visible
   text sits before it; assert today's reader returns `None`.
2. Replace the single fixed window with a **bounded backward scan**: widen in steps (e.g. 256 KiB →
   1 MiB → 4 MiB) until a visible entry is found or a hard ceiling is reached, so the common case
   still reads one small tail and the tool-heavy case still terminates.
3. Keep the redaction contract exactly: `tool_use` / `tool_result` bodies must still never surface,
   and `LAST_MESSAGE_MAX_CHARS` still bounds the cell.
4. Record the ceiling as a stated property (a session whose visible text is older than the ceiling
   still reads null — deliberately, and documented) rather than scanning unbounded.

## Quality Gate

**Acceptance criteria**

- A transcript whose 256 KiB tail holds only tool traffic surfaces the visible text that precedes it.
- A transcript with no visible text anywhere still yields `Null` (no invention).
- A `tool_result` body never surfaces (the existing test stays green unchanged).
- Reading a multi-megabyte transcript stays bounded — the scan never reads the whole file without a
  ceiling.

**Verification method**

- New unit tests in `packages/qfs/crates/qfs/src/claude.rs` covering all four criteria.
- `cargo test --workspace` green.
- Live: the `/claude/sessions` endpoint over this container's real store returns a non-empty
  `last_message` for the driving session.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` green, and the live check above passes.

## Considerations

- The mission gate cannot be ticked until this lands: the endpoint, the store read and the
  self-inclusion all already hold — the non-empty `last_message` clause is the only unmet half
  (`packages/qfs/crates/qfs/src/claude.rs`).
- Widening the window costs read time on every scan of every session. Step widening (rather than one
  big window) keeps the common case cheap; measure before picking the steps.
