---
created_at: 2026-08-01T06:15:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission:
---

# The shipped Chatwork messages view reads UNREAD-only, so a second read of the same room is empty

## Overview

`crates/skill/assets/examples/chatwork.qfs` declares

```qfs
CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages |> …
```

against `GET /v2/rooms/{room}/messages` **without the `force` query parameter**. That endpoint's
default is *unread messages only*: the first read returns rows, and every subsequent read of the
same room returns nothing until someone posts again. The cookbook article
(`docs/cookbook/chatwork.md`) presents the view as "the messages in a room", which is not what the
declaration asks the API for.

This was reported alongside ticket `20260727214856` (the form-codec / 400 fix) as a *related
observation, listed so it is not lost*. That ticket took the half that was a defect on any reading —
a `204 No Content` now decodes as zero rows instead of surfacing `invalid_path … http_decode`
(`crates/driver-http/src/applier.rs`, `decode`). **This half was deliberately left out of that PR-unit
because it changes what a shipped view returns, which is a declaration-semantics decision, not a bug
fix.**

## The decision to make

Passing `force=1` makes the view return the room's most recent messages on every read — the "latest
messages" reading a user expects, and the one the cookbook already describes. Against it: `force=1`
is not free on Chatwork's side (it is the heavier call, and the API docs mark the unread default as
the cheap path), and a caller who genuinely wants *what is new since I last looked* loses the only
spelling for it.

Three shapes, in order of preference to be ruled:

1. **`force=1` on the shipped view, and the cookbook stays as written.** One view, matching its
   documented meaning. The unread-only reading is then not expressible.
2. **Two declared views** — `…/messages` (forced, latest) and `…/messages/unread` — so both readings
   have a name and the cookbook teaches which is which.
3. **Leave the declaration and correct the cookbook** to say the view is unread-only. Cheapest, and
   honest, but it ships a "messages" view that answers nothing on a second read.

## Scope

1. Rule between the three shapes above.
2. Change `crates/skill/assets/examples/chatwork.qfs` accordingly.
3. Update `docs/cookbook/chatwork.md` so the article describes what the view actually returns, and
   regenerate the skill (`cargo run -p xtask -- gen-skills`).
4. Bump the plugin version in all four fields if the taught surface moves.

## Policies

- workaholic:implementation / honest-surfaces — a shipped declaration and the article that teaches it
  must describe what the statement actually returns. Today the article says "messages" and the
  declaration asks for "unread messages".
- workaholic:design / 「推測するな、宣言して拒否せよ」 — an empty second read is not a refusal and not an
  error; it is a correct answer to a question the user did not knowingly ask. The fix is to make the
  question visible, not to guess which one was meant.

## Quality Gate

- The declared view's request carries (or deliberately omits) `force` per the ruling, pinned by a
  hermetic test over the built request URL.
- `docs/cookbook/chatwork.md` describes the view's actual return, and
  `cargo run -p xtask -- gen-skills --check` is clean.
- `cargo test --workspace` green.
- Live confirmation (reading the same room twice returns rows both times) is an **attended** step —
  this environment has a live-connected Chatwork token and no unattended run may probe it.

## Notes

Split out of `20260727214856` on 2026-08-01 while driving that ticket's form-codec fix. The `204`
half of the same observation shipped with that PR-unit; this is the remainder.
