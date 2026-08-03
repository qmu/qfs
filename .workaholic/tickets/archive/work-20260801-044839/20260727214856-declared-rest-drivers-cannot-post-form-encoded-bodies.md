---
created_at: 2026-07-27T21:48:56+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: a-declared-write-resolves-a-name-the-way-a-query-does
claim: work-20260801-044839
---

# Declared REST drivers cannot POST form-encoded bodies, so the shipped Chatwork message INSERT always fails with 400

## Overview

`INSERT INTO /chatwork/rooms/{room}/messages` — the write shipped in
`crates/skill/assets/examples/chatwork.qfs` and documented in `docs/cookbook/chatwork.md` — is
broken against the live API. Every commit fails:

```
$ qfs run "insert into /chatwork/rooms/<room>/messages values (body) ('test')" --commit
{"error":{"code":"commit_failed","kind":"commit_failed",
 "message":"Terminal { reason: \"client error 400 for POST https://api.chatwork.com/v2/rooms/<room>/messages\" }"}}
```

Reproduced 2026-07-27 against two different rooms with a valid, connected token (reads on the
same connection work, so this is not auth). The cookbook's example statement is the failing one.

Cause: the declared map lowers to `INSERT INTO /http/chatwork/… VALUES (row)`, and the HTTP
driver serialises the row as a JSON request body. That endpoint — like most plain-REST APIs of
its generation — accepts parameters only as `application/x-www-form-urlencoded`, and answers a
JSON body with 400. There is no way to express "form-encode this row" in the declared DSL
today: `ENCODE` has `json`, `jsonl`, `yaml`, `toml`, `csv`, `md`, and `multipart`, but no form
codec.

So the gap is not Chatwork-specific. Any declared driver over a form-parameter REST API can
read but cannot write, which undercuts the blueprint §13 claim that such an API is expressible
as declarative config.

Workaround in use (verified live, message delivered): route the write through the multipart
encoder instead, which the endpoint does accept.

```
CREATE MAP INSERT /chatwork/rooms/{room}/messages_form AS
  INSERT INTO /http/chatwork/rooms/{room}/messages |> ENCODE multipart VALUES (row);
```

That works but is the wrong shape to ship — it sends a multipart body for a request that has no
file part, and it relies on the server being lenient.

## Scope

1. Add a form codec to `ENCODE` — `|> ENCODE form` — producing
   `application/x-www-form-urlencoded` from a flat row struct (scalars percent-encoded as
   `k=v&k=v`; decide and document the behaviour for nested/bytes fields rather than leaving it
   implicit).
2. Have the HTTP driver set `content-type: application/x-www-form-urlencoded` for that body, the
   same way `multipart.rs` already carries its own content type.
3. Fix the shipped declaration in `crates/skill/assets/examples/chatwork.qfs` to use it, and
   update `docs/cookbook/chatwork.md` so the documented example is one that actually commits.

## Related observation (separate from the above, listed so it is not lost)

Reading the same driver's messages twice surfaces a misleading error. The declared view calls
the endpoint without `force`, so the API returns only unread messages and answers `204 No
Content` once there are none. qfs reports that as:

```
{"error":{"code":"invalid_path","kind":"usage",
 "message":"invalid path \"/rest/chatwork/rooms/<room>/messages\": http_decode",
 "path":"/rest/chatwork/rooms/<room>/messages"}}
```

A `204` is a successful empty response; surfacing it as `invalid_path` sends the reader looking
for a typo in a path that is correct. Two things worth deciding: whether the generic HTTP read
should decode `204` as zero rows instead of a decode failure, and whether the shipped view
should pass `force=1` so the documented read is the "latest messages" one a user expects.

## Policies

- workaholic:implementation / honest-surfaces — a shipped declaration and the cookbook article that
  teaches it must describe a statement the binary can actually commit. Today `chatwork.qfs` and
  `docs/cookbook/chatwork.md` both publish an INSERT that always answers 400, which is the same
  defect class three sibling branches corrected in this release batch.
- workaholic:design / 「推測するな、宣言して拒否せよ」 — decide and document what `ENCODE form` does with
  nested and bytes fields rather than leaving it implicit; an unencodable field is refused, never
  silently flattened or dropped.
- Blueprint §13 — the claim under test is that a plain-REST API is expressible as declarative
  config. A form-parameter API that can be read but not written is a counter-example to that claim,
  so this is a gap in the strategy's premise rather than a Chatwork quirk.

## Quality Gate

- `insert into /chatwork/rooms/<room>/messages values (body) ('…')` — the statement printed in
  the cookbook — commits against the live API and the message appears in the room.
- The form codec is covered by a unit test on the encoder (field ordering, percent-encoding of
  reserved and multi-byte characters, the content-type header) and by a parse test for
  `|> ENCODE form`.
- `cargo test --workspace` green.
- If the `204` item is taken in the same change, a read that returns `204` yields an empty row
  set rather than an error, with a test pinning it.

## Notes

Filed from another repository via `/request`. Happy to supply the failing/working transcripts
in full if useful; the reproduction needs only a connected token and any room.
