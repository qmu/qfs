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

# A named launch captures the session NAME as its id, because the banner grew a third field

## Overview

`ClaudeCliLauncher::launch` reads the new session's id out of the `claude --bg` banner with
`parse_backgrounded_id`, which finds the line whose first token is `backgrounded` and returns that
line's **last** whitespace token. That was correct for the recorded 2.1.217 banner:

```
backgrounded · eb5300ad
```

Claude Code **2.1.233** appends the session name when `--name` is passed. Observed live in the
container on 2026-08-16, driving the ticket `20260805113300` launch round:

```
$ claude --bg "Do nothing at all. …" --name spike-solo
Starting background service…
backgrounded · 4f89081e · spike-solo          <-- three fields, not two
```

`LaunchSpec.name` is exactly what `INSERT INTO /claude/sessions (cwd, prompt, name)` puts on
`--name`, so **every named launch returns `spike-solo` where the id `4f89081e` was meant**. Without
`--name` the banner is still two fields and the parse is still right — which is why the live round
still passed its own gate: the id is discarded today (`applier.rs` calls `launcher.launch(&spec)?`
and returns an affected count of 1), so the wrong value never surfaces. It is a **latent** defect
that becomes a wrong answer the moment `RETURNING id` carries the launcher's id to a caller.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — all code work.
- `workaholic:implementation` / `policies/directory-structure.md` — all code work.
- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — a banner whose shape is not the one recorded
  should be refused, not silently reinterpreted. "Last token" was a guess that happened to hold for
  one release; a parse anchored on the separator or on the id's own shape states what it expects.

## Key Files

- `packages/qfs/crates/qfs/src/claude.rs` — `parse_backgrounded_id` (the parser) and
  `ClaudeCliLauncher::launch` (its only caller).
- `packages/qfs/crates/qfs/src/claude.rs` — `parse_backgrounded_id_reads_the_recorded_banner`, the
  test pinning the 2.1.217 two-field banner; it stays green because it never had a `--name` case.
- `packages/qfs/crates/driver-claude/src/applier.rs` — where the launched id is currently dropped.

## Implementation Steps

1. Reproduce: add a test over the observed three-field banner
   (`backgrounded \u{b7} 4f89081e \u{b7} spike-solo`) and watch it return `spike-solo`.
2. Anchor the parse on position rather than "last token": the id is the token **after** the first
   separator following `backgrounded`. Keep the two-field banner working (the existing test must
   stay green unchanged).
3. Refuse rather than guess when the line matches neither shape — `LaunchFailed` already exists and
   is what a caller can act on.
4. Decide, and record, whether the captured id should now surface through `RETURNING id`; if it
   should not yet, say so where the id is dropped so the next reader does not re-find this.

## Quality Gate

**Acceptance criteria**

- The three-field banner yields `4f89081e`, not `spike-solo`.
- The two-field 2.1.217 banner still yields `eb5300ad` (no regression).
- A `backgrounded` line matching neither shape yields `None`, so `launch` fails closed.

**Verification method**

- New unit tests in `packages/qfs/crates/qfs/src/claude.rs` covering all three cases.
- `cargo test --workspace` green.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check` all green.

## Considerations

- The banner is somebody else's output format and has now changed twice. Whatever the parse becomes,
  it should fail loudly on an unrecognised shape rather than return a plausible-looking wrong token —
  that is precisely how this defect stayed invisible (`packages/qfs/crates/qfs/src/claude.rs`).

## Final Report

Development completed as planned.

### Changes

`parse_backgrounded_id` stopped being "find the `backgrounded` line, take its last token" and became
a positional read of two stated shapes:

```text
backgrounded · eb5300ad                 → eb5300ad   (2.1.217, and 2.1.233 without --name)
backgrounded · 4f89081e · spike-solo    → 4f89081e   (2.1.233 with --name)
```

The id is the token **after the first separator**. A `backgrounded` line of any other shape yields
`None`, so `ClaudeCliLauncher::launch` raises `LaunchFailed` rather than returning a plausible
token. Separator identity is not asserted (`is_banner_separator` asks only that a token carries no
alphanumeric character), so the parse survives a change of glyph while still refusing a change of
*shape* — which is the change that actually happened, twice.

One structural change beyond the ticket's letter: the function now takes the **first**
`backgrounded` line and either parses it or refuses. Before, a malformed line returned `None` from
the `find_map` closure and the scan silently continued to later lines. That is the same
"keep looking until something plausible turns up" behaviour in a different place, and it would have
undone the fail-closed criterion.

Step 4's decision, recorded at `driver-claude/src/applier.rs` where the id is dropped: **the id
does not surface through `RETURNING id` yet.** `EffectOutcome` carries an affected count and no
row, so carrying the id to a caller is a change to the effect channel's shape, not to that call
site. The comment says so explicitly, and says why the defect was invisible — this very line
discards the value.

### Quality gate

| Criterion | Result |
| --- | --- |
| The three-field banner yields `4f89081e`, not `spike-solo` | Pass — `parse_backgrounded_id_reads_the_named_launch_banner`, the observed bytes verbatim. |
| The two-field banner still yields `eb5300ad` | Pass — `parse_backgrounded_id_reads_the_recorded_banner` is unchanged and green. |
| A `backgrounded` line matching neither shape yields `None` | Pass — `parse_backgrounded_id_unrecognised_shape_is_none` covers a six-field line, a separator-less line, and the bare keyword. |
| `cargo test --workspace`, clippy, fmt green | clippy (all three invocations) and fmt green; the suite carries one pre-existing racy failure unrelated to this ticket, ticketed as `20260818060942` and characterised there. |

### Discovered Insights

- **Insight**: the defect was invisible because the value was discarded. `applier.rs` throws the
  launcher's return away, so a wrong id cost nothing and the live-fire round that exercised this
  exact path passed its own gate while returning `spike-solo`.
  **Context**: a value nobody reads is not tested by anything that reads it. When the effect channel
  grows a `RETURNING` row, every producer feeding it deserves the same second look — being
  currently-discarded is not evidence of being correct.

- **Insight**: "the last token" is a shape assumption disguised as a position. It was right for a
  two-field banner and became a *name* reader the moment a third field appeared, with no error
  anywhere.
  **Context**: when parsing somebody else's output, anchor on the field you want, not on an end of
  the line — an appended field is the most common way an external format changes, and it is exactly
  the change that "last token" cannot survive.
