---
created_at: 2026-07-04T15:55:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 4h
commit_hash: e8bcc7c
category: Changed
depends_on: []
---

# Gmail: set-wide REMOVE/UPDATE by predicate on label collections fail at commit (describe lies)

Surfaced while driving **20260703150100 mail-drafts-write-parity** (owner de-scoped the broader leg
to this follow-up so #4 stays drafts-only). The drafts half is fixed; this is the systemic sibling.

## The gap

The Gmail applier decodes **purely** (`GmailEffect::from_node`) — it has **no apply-time
enumeration seam** like the Drive driver's `WriteResolver`. So a collection-level write whose target
is a `/mail/<label>` (not a single `id:<msg>`) cannot resolve which messages it acts on, and:

1. **Set-wide REMOVE** — `remove /mail/inbox where subject LIKE '%unsubscribe%'` (and the
   `where from == …` sender form) lowers to a single `Remove` effect node on `/mail/inbox`.
   `decode_trash` services only `MailPath::Message`/`Thread`, so the collection falls to
   `_ => CapabilityDenied` **at COMMIT**, while `describe /mail/inbox` advertises `REMOVE`. The
   cookbook's "Clean up — trash" recipes (`docs/cookbook/gmail.md`, trash-by-subject /
   trash-by-sender on `/mail/inbox`) preview fine and fail on `--commit`.
2. **Relabel by non-id filter** — `update /mail/inbox set add_labels = 'STARRED' where from ==
   'boss@example.com'` produces args with the eq-key column `from`, but `decode_modify_labels`
   requires an `id` column (`"UPDATE needs the target message id"`), so it also fails at COMMIT.
   Only `where id == '<msgid>'` carries the needed key. The cookbook's relabel-by-sender recipes are
   affected.

Both break the "describe never lies about verbs / preview and apply agree" contract for every
`/mail/<label>` node — the same contract #4 restored for `/mail/drafts`.

## Design note (carried from #4)

Even a Drive-style enumerator only services an **equality key** (`collect_eq_constants` in
`crates/core/src/eval.rs` carries only `col == const` leaves to the effect node; a `LIKE`/`OR`/range
predicate reaches the applier as **no** key). Drive's `remove_target_id` handles exactly one
`name == 'exact'` and fails closed otherwise. So the honest ceiling without a scan-based lowering is
`REMOVE/UPDATE <collection> WHERE <key> == <value>`; richer predicates must fail closed (not silently
match nothing).

## Fix (pick one, reconcile every /mail node)

- **A — implement apply-time enumeration.** Add a resolver seam to `GmailApplier` (mirror
  `driver-gdrive`'s `WriteResolver`/`ClientResolver`): for a collection `REMOVE`/`UPDATE` carrying an
  equality key, list matching messages (the `q=` search the read path already builds) and trash /
  modify each. Fail closed on a non-equality predicate. Rewrite the cookbook trash/relabel recipes to
  the equality forms that commit (`where subject == …` / seed by id), and keep `LIKE` out of write
  filters.
- **B — make caps/cookbook honest now.** Drop `Remove` from the `/mail/<label>` collection caps and
  narrow `UPDATE` to the id-keyed form; rewrite the affected cookbook recipes to committable shapes
  (single-message `remove /mail/inbox/<id>`, relabel `where id == …`). Fastest; narrows the triage
  surface the gmail cookbook's intro promises.

Either way: **preview and apply must agree for every `/mail` node**, and the cookbook must show only
forms that commit (the `crates/test/tests/cookbook_skills.rs` ratchet checks parse only, so a
live/e2e check or an effect-decode unit test must cover the committable shapes).

## Key files

- `packages/qfs/crates/driver-gmail/` (`applier.rs`, `effect.rs` `decode_trash`/`decode_modify_labels`,
  `lib.rs` `caps_for`), `crates/core/src/eval.rs` (`setwhere_row_batch`/`collect_eq_constants`),
  `docs/cookbook/gmail.md`.

## Quality Gate

- `describe` verb claims match what the applier accepts for **every** `/mail` node (labels included).
- The cookbook's trash and relabel recipes commit successfully (hermetic effect-build/enumeration
  unit tests, plus an owner-authorized live check if enumeration lands).
