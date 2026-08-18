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

## Final Report

Development completed as planned.

### The decision, and why the other option lost

**`status` is now nullable, and `"unknown"` is gone.** Null means *this store recorded no state for
this session*, which is a different fact from any status string and is the only fact the reader
actually has.

The alternative — deriving a status for interactive records from a field that is present (`kind`,
`statusUpdatedAt`, process liveness) — was rejected because every candidate answers a different
question than the column asks. `kind` says how the session was started; liveness is already the
filter that decides which rows exist at all. Synthesising `"running"` from "the pid is alive" would
put a value in the column that Claude Code never wrote, which is the sentinel problem again with a
better-chosen word. 「推測するな、宣言して拒否せよ」 reads directly onto that: declare the absence.

**Versioned surface.** Widening a non-nullable column to nullable is a change to a published
relation schema, so it is deliberate and it is what the patch bump on this PR covers. The direction
matters: the domain widens, so a caller that used to see `"unknown"` now sees null, and a caller
that used to see a real status still sees exactly that. No recorded value is reinterpreted.

### Changes

- `crates/qfs/src/claude.rs` — `SessionRecord.status` is `Option<String>`; the
  `unwrap_or_else(|| "unknown")` default is deleted; the row renders `Value::Null` for an absent
  status. The module doc's on-disk layout section now says which record shapes carry `status`, with
  the 2026-08-16 observation as its warrant, instead of listing it among the fields the store
  writes for every session.
- `crates/driver-claude/src/schema.rs` — `col("status", Text, true)`, with the reason on the line.
- `crates/driver-claude/src/lib.rs` — the two `WHERE status='running'` examples became
  `WHERE status == 'busy'`. `running` was never a value this store writes at all (`idle`/`busy`
  are), so the documented example selected nothing on *any* session, for a second reason nobody
  had noticed. The pushdown comment now also says the column is nullable and that such a predicate
  deliberately matches no interactive session.

### Quality gate

| Criterion | Result |
| --- | --- |
| A record with no `status` key is distinguishable from one whose status was recorded | Pass — `a_record_without_a_status_key_reads_null_not_a_sentinel` writes the observed interactive shape (no `status` key) and asserts `Value::Null`. |
| A record carrying `status` still surfaces it verbatim | Pass — `a_recorded_status_still_surfaces_verbatim`, plus the pre-existing `busy` assertion, unchanged. |
| `describe` and the scanned rows agree on nullability | Pass — `driver-claude`'s existing schema-agreement assertion compares `batch.schema` to `describe`'s and is green against the new schema. |
| `gen-docs --check` clean | Pass — and it produced no diff: the generated driver catalogue renders per-node capability, not per-column nullability, so this schema change does not reach `docs/drivers.md`. |
| clippy / fmt green | Pass, across all three clippy invocations. |

### Discovered Insights

- **Insight**: the documented example was wrong twice over. `WHERE status='running'` could not
  match an interactive session (no `status` key), and could not match a background one either —
  the store writes `idle`/`busy`, never `running`.
  **Context**: an example in a comment is unexecuted documentation. This one had been carried
  through the driver comment, the schema comment and `describe` output while matching nothing on
  any machine; only reading two real records side by side exposed it.

- **Insight**: a non-nullable column forces the reader to invent a value, and the invented value is
  always *shaped like* a real one. `"unknown"` is indistinguishable, to a filter, from a state the
  store genuinely called unknown.
  **Context**: nullability is not a storage detail here, it is the difference between "the answer is
  X" and "there is no answer" — the choice belongs at schema-design time, before a sentinel has to
  be picked.
