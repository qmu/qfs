---
created_at: 2026-08-16T16:11:43+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
merge_policy: review
verification_handoff:
---

# `status` reads `unknown` for every interactive session, so `where status == …` filters nothing

## Overview

`/claude/sessions` declares `status` **non-nullable** and the reader defaults a missing field to the
string `"unknown"`. Claude Code 2.1.233 writes `status` only for **background** sessions. The two
records observed side by side on 2026-08-16:

```
# interactive (this session, harness-launched) — no status key at all
{"pid":521,"sessionId":"cdb3f812-…","cwd":"/home/user/qfs","version":"2.1.233",
 "kind":"interactive","entrypoint":"remote_trigger","name":"qfs-5c","messagingSocketPath":"…"}

# background (launched by INSERT INTO /claude/sessions) — status present
{"pid":19142,"sessionId":"c0bbb006-…","kind":"bg","name":"live-fire",
 "status":"idle","statusUpdatedAt":1786896348…}
```

So the documented use `WHERE status = 'running'` (driver crate docs, `claude_node_schema` comment,
`describe` output) selects nothing on an interactive session and works only on background ones. The
column reads as data the store records for every session; it is not.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — all code work.
- `workaholic:implementation` / `policies/directory-structure.md` — all code work.
- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — `"unknown"` is a **string that looks like a
  status**. A caller filtering on it cannot tell "this session is in an unknown state" from "this
  store does not record a state for this kind of session".

## Key Files

- `packages/qfs/crates/qfs/src/claude.rs` — `read_session_record` (the `"unknown"` default) and the
  module doc's on-disk layout section, which lists `status` as a field the store writes.
- `packages/qfs/crates/driver-claude/src/schema.rs` — `claude_node_schema(Sessions)`, where `status`
  is declared non-nullable.
- `packages/qfs/crates/driver-claude/src/lib.rs` — the `WHERE status='running'` example in the
  pushdown comment.

## Implementation Steps

1. Reproduce with two fixture records — one carrying `status`, one not — and assert today's reader
   renders the second as `"unknown"`.
2. Decide the surface and record the reasoning: either make `status` **nullable** (absent means the
   store recorded none — the honest reading, and a schema change `describe` must reflect), or derive
   a status for interactive records from a field that is actually present. Do not keep a sentinel
   string in a non-nullable column.
3. Apply the choice to the schema, the reader, the module doc's layout list, and the
   `WHERE status='running'` examples so none of them can disagree.
4. Regenerate the reference docs (`cargo run -p xtask -- gen-docs`) if the schema moves.

## Quality Gate

**Acceptance criteria**

- A record with no `status` key is distinguishable from one whose status the store actually recorded.
- A record carrying `status` still surfaces that value verbatim.
- `describe /claude/sessions` and the rows the scan returns agree on the column's nullability.

**Verification method**

- New unit tests in `packages/qfs/crates/qfs/src/claude.rs`; the schema-agreement assertion in
  `packages/qfs/crates/driver-claude/src/lib.rs` covers the third criterion.
- `cargo run -p xtask -- gen-docs --check` clean.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check` all green.

## Considerations

- Making the column nullable is a change to a **published relation schema**, which the README's
  SemVer policy treats as versioned surface — take that into account when choosing between the two
  options (`packages/qfs/crates/driver-claude/src/schema.rs`).
