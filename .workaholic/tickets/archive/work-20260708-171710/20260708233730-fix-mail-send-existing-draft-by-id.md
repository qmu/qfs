---
created_at: 2026-07-08T23:37:30+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, Infrastructure]
effort:
commit_hash: 3c04692
category: Changed
depends_on:
mission:
---

# Fix: CALL mail.send cannot send an existing Gmail draft (falls to a byteless create-then-send)

## Overview

Sending a Gmail draft/reply through qfs fails with `draft has no `to` recipients`. Reproduced live on
2026-07-08 against a connected `/mail` account, on both the installed `qfs 0.0.29` and a fresh build
of the current tree (`0.0.33`).

**Root cause (verified in source):** `CALL mail.send` reads the draft id **only from a `draft_id`
row column** (`packages/qfs/crates/driver-gmail/src/effect.rs:222`, `decode_call`), and **never from
the addressed path** `/mail/drafts/<id>`. But:

- the `/mail/drafts` read schema exposes `id` (message-shaped: `id, thread_id, date, from, subject,
  snippet, label_ids, attachments`) — there is **no `draft_id` column** to feed the send; and
- a message-node path `/mail/drafts/<id> |> call mail.send` does not lower the path segment into a
  `draft_id` column.

So every query form falls into the **create-then-send** branch (`draft_id: None`, build a draft from
the row via `draft_from_row`). The upstream row for a `/mail/drafts` read is message-shaped and
carries no `to`/`body`, so `draft_from_row` yields an empty-recipient draft and the applier refuses
with `malformed INSERT effect … draft has no `to` recipients`. **There is currently no working query
form to send an existing draft by id**, which makes the cookbook's create-then-send recipe
(`insert into /mail/drafts …; /mail/drafts |> call mail.send`) non-functional.

**Not the cause (ruled out during diagnosis):** the named-column INSERT of `to` is fine — the parser
captures `VALUES (to, subject)` as columns `["to","subject"]`
(`packages/qfs/crates/parser/src/tests.rs:268` `insert_values_returning`) and a core eval test
asserts the lowered node's `column_names()` includes `to`
(`packages/qfs/crates/core/src/eval/tests.rs`). So the draft created by
`insert into /mail/drafts values (to, subject, body) (...)` almost certainly DID carry the recipient;
the failure is entirely in the SEND path.

## Reproduction

```sh
# 1) create a draft (this part is fine — the draft carries `to`)
qfs run "insert into /mail/drafts values (to, subject, body) ('someone@example.com', 'Re: test', 'hello')" --commit

# 2) try to send it — every form below fails with "draft has no `to` recipients":
qfs run "/mail/drafts/<draft-id> |> call mail.send" --commit --commit-irreversible
qfs run "/mail/drafts |> where id == '<draft-id>' |> select id as draft_id |> call mail.send" --commit --commit-irreversible
```

Observed: `{"error":{"code":"commit_failed", ... "malformed INSERT effect at \"/mail/drafts\": draft has no `to` recipients"}}`.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — keep the fix in the owning
  seams: the gmail driver effect decode (`driver-gmail`), the drafts read schema, and (if a path
  form is chosen) the effect lowering; no new top-level areas.
- `workaholic:implementation` / `policies/coding-standards.md` — typed, explicit id plumbing; the
  draft id must reach the effect as a typed column, not by silent fall-through.
- `workaholic:implementation` / `policies/objective-documentation.md` — the Gmail cookbook send
  recipe must be **true against the running binary**, not merely parse-checked; whichever form ships
  is the one the cookbook teaches.
- `workaholic:implementation` / `policies/test.md` — add the missing end-to-end (query string →
  plan → mock applier) coverage so a parse-check-only gap can't hide a non-functional send again.
- `workaholic:design` / `workaholic:safety` — `mail.send` stays irreversible; PREVIEW must not send;
  a send that cannot resolve a real draft/recipient must fail closed at PLAN time (so preview and
  apply agree), not silently create-then-send an empty draft at COMMIT.

## Key Files

Verified anchors at HEAD `24b1ef2` (2026-07-08):

- `packages/qfs/crates/driver-gmail/src/effect.rs:216-231` — `decode_call`: reads `draft_id` only
  from `DRAFT_ID_COL`; add resolution of the draft id from the addressed message-node path
  `/mail/drafts/<id>` (mirror how `decode_trash`/label-update read `MailPath::Message { id }`), OR
  define the supported send-existing-draft column and wire the read/pipeline to produce it.
- `packages/qfs/crates/driver-gmail/src/effect.rs:284` — `draft_from_row`: the create-then-send
  builder that silently accepts a recipient-less row; make an empty-recipient create-then-send fail
  at PLAN time with an actionable error rather than at COMMIT.
- `packages/qfs/crates/driver-gmail/src/path.rs` — `MailPath::Message { id }` under `/mail/drafts`;
  the path form a send-by-id should consult.
- `packages/qfs/crates/driver-gmail/src/read.rs` — the drafts read schema (exposes `id`, not
  `draft_id`); decide whether the send consumes `id` from an upstream draft row.
- `packages/qfs/crates/driver-gmail/src/lib.rs:216-233` — `plan_write`/capability seam and the
  existing "named columns" draft-write guard; the send-path guard belongs alongside it.
- `docs/cookbook/gmail.md:370-381` — the create-then-send recipe that must be made true (and the
  skill regenerated).
- `packages/qfs/crates/driver-gmail/src/tests.rs:663-687` — `call_mail_send_decodes_…` constructs the
  node directly with a `draft_id` column; it does NOT exercise a query string, which is why the gap
  survived. Extend with the end-to-end form.

## Related History

- `.workaholic/tickets/archive/…/` — the Gmail draft/send feature (t92 draft attachments, the
  `send` prelude alias). The send-by-id path was decode-tested but never query-tested end to end.
- Same class as the Codex-review finding on Slack DM file listings (2026-07-08): a cookbook recipe
  that parse-checks but is not true at runtime because no end-to-end test drives it.

## Implementation Steps

1. Decide the supported **send-existing-draft** form (pick one, definitively — experimental, no
   compat shims):
   - **path form** `/mail/drafts/<id> |> call mail.send` → `decode_call` reads the id from
     `MailPath::Message { id }` and emits `Send { draft_id: Some(id) }`; and/or
   - **pipeline form** where the drafts read yields a usable id the send consumes (align the read
     column name with what `decode_call` reads, or map it explicitly).
2. Make a recipient-less **create-then-send** fail closed at PLAN time (preview == apply), with an
   actionable message pointing at the correct send form — never a COMMIT-time `malformed INSERT`.
3. Update `docs/cookbook/gmail.md` to the working send form and regenerate the skill
   (`gen-skills`); keep the recipe parse-checked AND covered by the new e2e test.
4. Add end-to-end tests (below) with the mock Gmail client, driving the actual query strings.

## Quality Gate

**Acceptance criteria:**

- A draft created by `insert into /mail/drafts values (to, subject, body) (...)` can be **sent** by
  the documented form, and the mock applier records a send of THAT draft (by id) carrying the
  recipient — proven by a hermetic end-to-end test that runs the query string(s), not a
  hand-built node.
- A `mail.send` that resolves to no real draft/recipient **fails at PLAN time** (preview and apply
  agree), never a COMMIT-time `malformed INSERT … no to recipients`.
- `mail.send` remains irreversible; PREVIEW performs no send (asserted against the mock: zero send
  calls during preview).
- The Gmail cookbook send recipe is true against the binary (parse-checked AND e2e-tested).

**Verification method:**

- `cargo test -p qfs-driver-gmail` (new end-to-end query→send test + the plan-time fail-closed test).
- `cargo test -p qfs-test --test cookbook_skills` (recipe parses) and the new e2e test (recipe runs).
- `cargo run -p xtask -- gen-docs --check` / `gen-skills --check`; `cargo clippy --workspace
  --all-targets -- -D warnings`; `cargo fmt --all --check` (never piped).
- Manual live smoke (owner, out of band, one-shot): create a draft and send it to a throwaway
  address; confirm it lands. This dev host has LIVE cloud accounts connected
  (`.claude` memory: qfs-env-has-live-cloud-accounts) — keep any live check to an intended recipient
  and confirm before sending.

**Gate:** hermetic suites green + the four ratchets in sync; a draft round-trips create → send with
the recipient preserved; no COMMIT-time malformed-INSERT path remains.

## Considerations

- The `to` column is NOT the bug — do not "fix" the parser/lowering of named draft columns; the
  parse and core-eval tests already prove `to` is carried. Focus on the send path.
- `draft_from_row` accepting a recipient-less row is the trap that turns a wrong send form into a
  confusing COMMIT-time error; the plan-time guard is the durable fix (preview must not lie).
- Keep the reversible/irreversible split intact: creating a draft stays reversible; `mail.send` and
  draft removal stay irreversible.
- Commit via `workaholic:commit` `commit.sh` with explicit file args; never `git add -A`.
