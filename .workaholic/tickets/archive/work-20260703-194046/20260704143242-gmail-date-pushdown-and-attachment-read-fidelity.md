---
created_at: 2026-07-04T14:32:42+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 2h
commit_hash: 539cdab
category: Changed
depends_on: []
---

# Gmail read fidelity: date predicates silently no-op, and the `attachments` column is always empty

Live dogfooding of the Gmail driver (owner-authorized, 2026-07-04, CLI v0.0.17,
`/mail` mounted to the owner's account) while searching for a ~2–3-year-old sent
email. Two read-path defects made the task materially harder; both return
**plausible-but-wrong** results with no error, which is the dangerous kind.

## Finding 1 (primary) — `WHERE date …` is silently dropped, so time-range search is impossible

`build_query`'s date lowering only fires for an **integer (epoch-ms) literal**;
a date-**string** literal and `BETWEEN` match the `_ => None` arm — so nothing is
pushed to Gmail's `after:`/`before:`, and (observed live) nothing is residual-filtered
either. The predicate is **silently ignored** and the tail returns the newest N
rows as if no date filter were given.

Repro (each returned only 2026 mail — the newest, unfiltered):

```
/mail/sent |> where date BETWEEN '2023-01-01' AND '2024-12-31' |> select date, subject |> limit 500
/mail/sent |> where date < '2024-01-01' |> select date, subject |> limit 5
```

Root cause — `crates/driver-gmail/src/query.rs` (the lowering match, ~line 156):

```rust
("date", CmpOp::Gt | CmpOp::Ge, Literal::Int(ms)) => Some(Lowered::PreFilter(format!("after:{}",  ms / 1000))),
("date", CmpOp::Lt | CmpOp::Le, Literal::Int(ms)) => Some(Lowered::PreFilter(format!("before:{}", ms / 1000))),
_ => None,
```

- Only `Literal::Int` is handled — a `date`-typed **string** literal (`'2024-01-01'`)
  is not coerced, so it hits `_ => None`.
- `BETWEEN` is documented (module docblock, ~line 34) as "pushes nothing and stays
  residual" — but the residual re-check did not actually filter live, so the whole
  predicate vanished.

Impact: you cannot search mail by time window at all through qfs — the exact
operation a date column exists for. I had to fall back to `subject LIKE '%…%'`
(which *does* push down and works well across years) to find the message.

## Finding 2 (secondary) — the `attachments` column is always `[]` on message reads

`select attachments` over any label returns `[]` for **every** message, including
ones that provably carry files (confirmed via the Gmail API for the same message:
a PDF 見積書 + a PNG). So attachment presence/filenames are undetectable through qfs.

Root cause — `crates/driver-gmail/src/client.rs`: `get_message` fetches
`…/messages/{id}?format=metadata` (~line 233), but `decode_attachments` (~line 416)
harvests attachment parts from `payload.parts` — which `format=metadata` does **not**
return. So `decode_attachments` always yields an empty vec.

This is the message-read counterpart of the draft-side symptom in
`20260703150200-read-projection-fidelity.md` (#3, "draft attachments read back empty");
same `attachments` column, different code path. Fixing both together makes the column
trustworthy end-to-end.

## Fix

1. **Date lowering (query.rs):** accept a `date`/timestamp **string** literal by
   coercing it to epoch-ms before the `after:`/`before:` lowering, and lower
   `BETWEEN a AND b` on `date` to `after:<a> before:<b>`. Anything genuinely not
   pushable must be **residual-filtered locally** (never silently dropped) — a
   `WHERE` that qfs cannot honor should narrow the result, not be ignored.
2. **Attachments (client.rs):** fetch the parts needed for attachment metadata —
   use `format=full` for `get_message` (or a targeted parts fetch), so
   `decode_attachments` sees `payload.parts`. Metadata only (filename/mime/size);
   bytes stay lazy via `attachments.get`.
3. Reconcile the residual-vs-pushdown contract in the module docblock with actual
   behavior once (1) lands.

## Key files

- `packages/qfs/crates/driver-gmail/src/query.rs` — date/BETWEEN lowering + residual discipline.
- `packages/qfs/crates/driver-gmail/src/client.rs` — `get_message` fetch format, `decode_attachments`.
- `packages/qfs/crates/driver-gmail/src/read.rs` — message-listing row projection.
- `docs/cookbook/gmail.md` — the "date range" and "read one message's attachments" recipes.

## Quality Gate

- `where date BETWEEN 'YYYY-MM-DD' AND 'YYYY-MM-DD'` and `where date < 'YYYY-MM-DD'`
  return only messages in-range (mock-tested against a fixed clock; live-spot-checked).
- A `WHERE` on `date` that cannot be pushed still narrows the result locally — never
  returns rows outside the predicate.
- `select attachments` lists the real attachments (filename/mime/size) for a message
  known to carry them (hermetic mock test; one live confirmation by the owner).

## Considerations / cross-refs

- **Related, don't duplicate:** attachments overlaps `20260703150200-read-projection-fidelity.md`
  (draft side) — coordinate the fix. Also seen live and already ticketed: user labels
  listing as raw ids (`Label_5`) — same read-projection ticket #2.
- **Out of scope (already ticketed):** CLI/skill drift — the bundled `qfs-gmail` skill
  (v0.1.0) documents `qfs connection add …` / `CONNECT`, but the installed CLI (v0.0.17)
  uses `qfs app` / `qfs account` / `qfs connect`; see `20260703150400-plugin-cache-staleness.md`
  and `20260703150300-agent-facing-doc-gaps.md`.
- **UX note (agent/headless):** with `QFS_PASSPHRASE` unset and no TTY, even
  `qfs account list` fails to unlock; `qfs vault enroll keychain` is the fix but is
  easy to miss — worth surfacing in the headless/agent setup docs.
- **Positive:** `subject`/`from` `LIKE` pushdown is excellent — server-side, reaches
  mail across many years — and the `describe → query` loop is clean. The date gap is
  the one thing that broke an otherwise smooth search.
