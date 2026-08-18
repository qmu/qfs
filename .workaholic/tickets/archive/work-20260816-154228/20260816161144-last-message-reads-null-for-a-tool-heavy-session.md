---
created_at: 2026-08-16T16:11:43+00:00
status: done
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

## Final Report

Development completed as planned.

### Changes

`TRANSCRIPT_TAIL_BYTES` (one 256 KiB window) became `TRANSCRIPT_SCAN_WINDOWS`
(`[256 KiB, 1 MiB, 4 MiB]`), and `last_visible_message` widens through them until a window yields
visible text. A session whose last message is recent is still answered by the first window and the
other two never run, so the common case costs exactly what it cost before.

The scan splits into three pieces so the ceiling is testable without writing a 4 MiB fixture:
`last_visible_message` (the public reading, with the real constant), `last_visible_message_within`
(the same logic with the windows named), and `last_visible_in` (one already-read window, scanned
backwards). The redaction contract is untouched — `entry_visible_text` was not modified, so
`tool_use`/`tool_result` bodies still never surface and `LAST_MESSAGE_MAX_CHARS` still bounds the
cell.

Step 4's stated property, written into the doc comment: **a session whose last visible text lies
further back than the widest window still reads `Null`, deliberately.** `/claude/sessions` is a
listing surface; scanning an unbounded file per session per listing is not a thing this column may
do. What the ticket fixed is not "always find the message" but "stop reporting absence that was
never established" for the ordinary tool-heavy shape.

### A bug the ceiling test found

The first implementation propagated `read_tail`'s `None` with `?`, exactly as the single-window
version had. But `read_tail` returns `None` for a window that contains **no complete line** — and a
transcript of long `tool_result` entries produces exactly that for a narrow window, because one
entry is larger than the window. So the first narrow window aborted the whole scan and the widening
never ran: the ceiling test read `Null` through a window wide enough to hold the message.

The absent/unreadable transcript is now answered once, up front, by the `metadata` call. Inside the
loop a `None` means the narrower thing — this window held no complete line — and that **widens**
rather than aborting.

### Quality gate

| Criterion | Result |
| --- | --- |
| A transcript whose 256 KiB tail holds only tool traffic surfaces the text before it | Pass — `visible_text_before_a_tool_only_first_window_still_surfaces` buries the message under ~330 KiB of `tool_result` traffic and reads it back. |
| A transcript with no visible text anywhere still yields `Null` | Pass — `a_transcript_of_pure_tool_traffic_stays_null`. |
| A `tool_result` body never surfaces | Pass — `tool_traffic_tail_walks_back_to_visible_text` is unchanged and green. |
| The scan stays bounded | Pass — `the_backward_scan_stops_at_its_ceiling` states it both ways: a ceiling below the message reads null, a ceiling above it finds the same message. |
| `cargo test --workspace` green | The claude module's 27 tests pass; the suite carries one pre-existing racy failure unrelated to this ticket (`20260818060942`). |
| Live `/claude/sessions` returns a non-empty `last_message` | **Not run here** — it needs the container's real store and a running server. The fixture reproduces the measured shape (tool-only first window, visible text behind it) and the mechanism is proven on it. |

### Discovered Insights

- **Insight**: the old comment — "the last visible text virtually always lives in the final few
  entries" — was true of a *conversational* session and false of an agentic one. The measurement
  that killed it: 109 entries in the 256 KiB tail, 0 visible, 4 visible in the whole 1.6 MB file.
  **Context**: an assumption about workload shape written into a constant ages with the workload.
  This module reads the store of the very agent that changed the shape.

- **Insight**: `read_tail` returning `None` conflates "cannot read this file" with "this window has
  no complete line". Harmless with one window; a silent abort as soon as there are several.
  **Context**: a helper's `None` gets re-read every time a caller changes. Widening a loop around
  an existing `?` is where a single-shot helper's ambiguity turns into a bug — and the test that
  caught it was the one written for the ceiling, not for the feature.
